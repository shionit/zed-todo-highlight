mod keywords;
use keywords::{build_keywords, Keyword};

use lsp_server::{Connection, Message, Response};
use lsp_types::{
    notification::{
        DidChangeConfiguration, DidChangeTextDocument, DidCloseTextDocument,
        DidOpenTextDocument, Notification,
    },
    request::{Request as LspRequest, SemanticTokensFullRequest, SemanticTokensRangeRequest},
    MessageType, Range, SemanticToken, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensRangeResult, SemanticTokensResult, SemanticTokensServerCapabilities,
    ServerCapabilities, ShowMessageParams, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
    WorkDoneProgressOptions,
};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Text utilities
// ---------------------------------------------------------------------------

/// Converts an LSP `Position` (line, UTF-16 character) to a byte offset in `text`.
fn lsp_pos_to_byte(text: &str, line: u32, character: u32) -> usize {
    let mut current_line = 0u32;
    let mut line_start = 0usize;

    for (i, b) in text.bytes().enumerate() {
        if current_line == line {
            // Walk the line counting UTF-16 code units until we hit `character`.
            let line_text = &text[line_start..];
            let mut utf16_col = 0u32;
            for (j, ch) in line_text.char_indices() {
                if utf16_col >= character {
                    return line_start + j;
                }
                utf16_col += ch.len_utf16() as u32;
            }
            return line_start + line_text.len();
        }
        if b == b'\n' {
            current_line += 1;
            line_start = i + 1;
        }
    }
    text.len()
}

/// Applies a single incremental content-change event to `text` and returns
/// the updated document string. Handles both ranged and full-replacement events.
fn apply_text_change(
    text: &str,
    change: &lsp_types::TextDocumentContentChangeEvent,
) -> String {
    match change.range {
        Some(range) => {
            let start = lsp_pos_to_byte(text, range.start.line, range.start.character);
            let end = lsp_pos_to_byte(text, range.end.line, range.end.character);
            format!("{}{}{}", &text[..start], change.text, &text[end..])
        }
        // No range means a full-document replacement (fallback for FULL sync clients).
        None => change.text.clone(),
    }
}

// ---------------------------------------------------------------------------
// Token scanning
// ---------------------------------------------------------------------------

/// Raw match: (line, col_utf16, length_utf16, token_type_index).
type Hit = (u32, u32, u32, u32);

fn find_hits(text: &str, keywords: &[Keyword]) -> Vec<Hit> {
    let line_starts: Vec<usize> = {
        let mut v = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                v.push(i + 1);
            }
        }
        v
    };

    let byte_to_line_col = |byte_off: usize| -> (u32, u32) {
        let line = line_starts.partition_point(|&s| s <= byte_off).saturating_sub(1);
        let col_byte = byte_off - line_starts[line];
        let col_utf16 = text[line_starts[line]..line_starts[line] + col_byte]
            .encode_utf16()
            .count() as u32;
        (line as u32, col_utf16)
    };

    let mut hits: Vec<Hit> = Vec::new();
    for kw in keywords {
        for m in kw.pattern.find_iter(text) {
            let (line, col) = byte_to_line_col(m.start());
            let len_utf16 = m.as_str().encode_utf16().count() as u32;
            hits.push((line, col, len_utf16, kw.token_type_index));
        }
    }
    hits.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    hits
}

/// Delta-encodes sorted hits into LSP `SemanticToken`s (relative to doc origin).
fn delta_encode(hits: &[Hit]) -> Vec<SemanticToken> {
    let mut tokens = Vec::with_capacity(hits.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;
    for &(line, col, length, token_type) in hits {
        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 { col - prev_start } else { col };
        tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: 0,
        });
        prev_line = line;
        prev_start = col;
    }
    tokens
}

#[cfg(test)]
fn scan_tokens(text: &str, keywords: &[Keyword]) -> Vec<SemanticToken> {
    delta_encode(&find_hits(text, keywords))
}

#[cfg(test)]
fn scan_tokens_in_range(text: &str, keywords: &[Keyword], range: &Range) -> Vec<SemanticToken> {
    let start = (range.start.line, range.start.character);
    let end = (range.end.line, range.end.character);
    let filtered: Vec<Hit> = find_hits(text, keywords)
        .into_iter()
        .filter(|&(line, col, ..)| (line, col) >= start && (line, col) < end)
        .collect();
    delta_encode(&filtered)
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

struct Document {
    text: String,
    version: i64,
    /// Cached scan result, valid for the current `version`.
    /// `None` means the cache is cold and a full scan is needed.
    cached_hits: Option<Vec<Hit>>,
}

struct Server {
    base_keywords: Vec<Keyword>,
    /// Active keyword set: base + any extras from workspace config.
    keywords: Vec<Keyword>,
    /// Token type index used for user-added extra keywords.
    custom_token_type_index: u32,
    documents: HashMap<Url, Document>,
    // Setup-hint tracking — detect when semantic tokens are not enabled in Zed.
    documents_opened: usize,
    // Set to true once 2+ documents have been opened. The hint is only eligible
    // to fire after this point, and only on a non-didOpen message so that Zed's
    // initial batch of didOpen + semanticTokens/full messages is fully processed
    // before we conclude that semantic tokens are unconfigured.
    hint_eligible: bool,
    semantic_tokens_ever_requested: bool,
    setup_hint_sent: bool,
}

impl Server {
    fn new(base_keywords: Vec<Keyword>, legend: &SemanticTokensLegend) -> Self {
        let custom_token_type_index = legend
            .token_types
            .iter()
            .position(|t| t.as_str() == "xxxKeyword")
            .unwrap_or(8) as u32;
        Self {
            keywords: base_keywords.clone(),
            base_keywords,
            custom_token_type_index,
            documents: HashMap::new(),
            documents_opened: 0,
            hint_eligible: false,
            semantic_tokens_ever_requested: false,
            setup_hint_sent: false,
        }
    }

    /// Returns true when the hint should be sent: at least 2 documents were opened,
    /// no semantic token request has ever arrived, and the hint hasn't been sent yet.
    ///
    /// Checked only on non-didOpen messages so Zed's initial batch of
    /// `didOpen` + `semanticTokens/full` messages can fully arrive before we
    /// conclude that semantic tokens are unconfigured.
    fn should_send_setup_hint(&self) -> bool {
        self.hint_eligible && !self.semantic_tokens_ever_requested && !self.setup_hint_sent
    }

    /// Returns the setup hint notification if it is due, marking it as sent.
    ///
    /// Call this after processing any non-didOpen message.
    fn take_hint_if_needed(&mut self) -> Option<Message> {
        if !self.should_send_setup_hint() {
            return None;
        }
        self.setup_hint_sent = true;
        Some(Message::Notification(lsp_server::Notification {
            method: "window/showMessage".to_string(),
            params: serde_json::to_value(ShowMessageParams {
                typ: MessageType::INFO,
                message: "TODO Highlighter: keyword highlighting is not active. \
                    Add \"semantic_tokens\": \"combined\" and \
                    a semantic_token_rules block to your Zed settings (⌘,). \
                    The README has a complete copy-paste block \
                    including optional background colors."
                    .to_string(),
            })
            .unwrap(),
        }))
    }

    /// Returns full semantic tokens, using the per-document hit cache when warm.
    fn tokens_for(&mut self, uri: &Url) -> SemanticTokens {
        self.semantic_tokens_ever_requested = true;
        let keywords = &self.keywords;
        let data = self.documents.get_mut(uri).map(|doc| {
            let hits = doc.cached_hits.get_or_insert_with(|| find_hits(&doc.text, keywords));
            delta_encode(hits)
        }).unwrap_or_default();
        SemanticTokens { result_id: None, data }
    }

    /// Returns range-filtered semantic tokens, re-using the cached hit list.
    fn tokens_for_range(&mut self, uri: &Url, range: &Range) -> SemanticTokens {
        self.semantic_tokens_ever_requested = true;
        let keywords = &self.keywords;
        let start = (range.start.line, range.start.character);
        let end = (range.end.line, range.end.character);
        let data = self.documents.get_mut(uri).map(|doc| {
            let hits = doc.cached_hits.get_or_insert_with(|| find_hits(&doc.text, keywords));
            let filtered: Vec<Hit> = hits
                .iter()
                .copied()
                .filter(|&(line, col, ..)| (line, col) >= start && (line, col) < end)
                .collect();
            delta_encode(&filtered)
        }).unwrap_or_default();
        SemanticTokens { result_id: None, data }
    }

    /// Applies workspace configuration sent via `workspace/didChangeConfiguration`.
    ///
    /// Expected settings shape:
    /// ```json
    /// { "extraKeywords": ["IDEA", "QUESTION", "REVIEW"] }
    /// ```
    /// Extra keywords use the `xxxKeyword` token type (purple) by default.
    fn apply_config(&mut self, settings: &serde_json::Value) {
        let extras: Vec<String> = settings
            .get("extraKeywords")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.trim().to_uppercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let custom_index = self.custom_token_type_index;
        let custom: Vec<Keyword> = extras
            .iter()
            .filter_map(|word| {
                regex::Regex::new(&format!(r"\b{}\b", regex::escape(word)))
                    .ok()
                    .map(|pattern| Keyword { pattern, token_type_index: custom_index })
            })
            .collect();

        self.keywords = self.base_keywords.iter().cloned().chain(custom).collect();

        // Keyword set changed — invalidate all cached hits.
        for doc in self.documents.values_mut() {
            doc.cached_hits = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Server run loop
// ---------------------------------------------------------------------------


/// Pulls configuration from the client and then processes all incoming
/// messages until the connection closes or a shutdown request is received.
fn run(connection: Connection, mut server: Server) {
    // Zed delivers `lsp.<server>.settings` in response to workspace/configuration
    // requests; it does NOT send them via initializationOptions on startup.
    const CONFIG_REQ_ID: i32 = 1;
    // Sent after apply_config to ask Zed to re-request tokens for all open documents.
    const REFRESH_REQ_ID: i32 = 2;
    connection
        .sender
        .send(Message::Request(lsp_server::Request {
            id: lsp_server::RequestId::from(CONFIG_REQ_ID),
            method: "workspace/configuration".to_string(),
            params: serde_json::json!({ "items": [{ "section": "todo-highlighter-lsp" }] }),
        }))
        .unwrap();

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req).unwrap() {
                    break;
                }

                let resp = match req.method.as_str() {
                    SemanticTokensFullRequest::METHOD => {
                        match serde_json::from_value::<lsp_types::SemanticTokensParams>(req.params)
                        {
                            Ok(params) => {
                                let tokens = server.tokens_for(&params.text_document.uri);
                                Response::new_ok(req.id, SemanticTokensResult::Tokens(tokens))
                            }
                            Err(e) => Response::new_err(
                                req.id,
                                lsp_server::ErrorCode::InvalidParams as i32,
                                format!("invalid params: {e}"),
                            ),
                        }
                    }

                    SemanticTokensRangeRequest::METHOD => {
                        match serde_json::from_value::<lsp_types::SemanticTokensRangeParams>(
                            req.params,
                        ) {
                            Ok(params) => {
                                let tokens = server
                                    .tokens_for_range(&params.text_document.uri, &params.range);
                                Response::new_ok(req.id, SemanticTokensRangeResult::Tokens(tokens))
                            }
                            Err(e) => Response::new_err(
                                req.id,
                                lsp_server::ErrorCode::InvalidParams as i32,
                                format!("invalid params: {e}"),
                            ),
                        }
                    }

                    _ => Response::new_err(
                        req.id,
                        lsp_server::ErrorCode::MethodNotFound as i32,
                        format!("unknown request: {}", req.method),
                    ),
                };

                connection.sender.send(Message::Response(resp)).unwrap();
                // Checked here (not inside didOpen) so Zed's initial didOpen batch
                // has already been followed by any semanticTokens/full requests before
                // we judge whether semantic tokens are configured.
                if let Some(hint) = server.take_hint_if_needed() {
                    let _ = connection.sender.send(hint);
                }
            }

            Message::Notification(notif) => match notif.method.as_str() {
                DidOpenTextDocument::METHOD => {
                    match serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(
                        notif.params,
                    ) {
                        Ok(params) => {
                            server.documents.insert(
                                params.text_document.uri,
                                Document {
                                    text: params.text_document.text,
                                    version: params.text_document.version as i64,
                                    cached_hits: None,
                                },
                            );
                            server.documents_opened += 1;
                            if server.documents_opened >= 2 {
                                server.hint_eligible = true;
                            }
                        }
                        Err(e) => eprintln!("todo-highlighter-lsp: bad didOpen params: {e}"),
                    }
                    // No hint check here — wait for a non-didOpen message so that
                    // the semanticTokens/full requests Zed sends after the didOpen
                    // batch can arrive first.
                }

                DidChangeTextDocument::METHOD => {
                    match serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(
                        notif.params,
                    ) {
                        Ok(params) => {
                            if let Some(doc) = server.documents.get_mut(&params.text_document.uri)
                            {
                                for change in params.content_changes {
                                    doc.text = apply_text_change(&doc.text, &change);
                                }
                                doc.version = params.text_document.version as i64;
                                // Invalidate cache — text changed.
                                doc.cached_hits = None;
                            }
                        }
                        Err(e) => eprintln!("todo-highlighter-lsp: bad didChange params: {e}"),
                    }
                    if let Some(hint) = server.take_hint_if_needed() {
                    let _ = connection.sender.send(hint);
                }
                }

                DidCloseTextDocument::METHOD => {
                    match serde_json::from_value::<lsp_types::DidCloseTextDocumentParams>(
                        notif.params,
                    ) {
                        Ok(params) => {
                            server.documents.remove(&params.text_document.uri);
                        }
                        Err(e) => eprintln!("todo-highlighter-lsp: bad didClose params: {e}"),
                    }
                    if let Some(hint) = server.take_hint_if_needed() {
                    let _ = connection.sender.send(hint);
                }
                }

                DidChangeConfiguration::METHOD => {
                    // Zed sends this as a hint that settings changed; re-pull the actual values.
                    let _ = connection.sender.send(Message::Request(lsp_server::Request {
                        id: lsp_server::RequestId::from(CONFIG_REQ_ID),
                        method: "workspace/configuration".to_string(),
                        params: serde_json::json!({
                            "items": [{ "section": "todo-highlighter-lsp" }]
                        }),
                    }));
                    if let Some(hint) = server.take_hint_if_needed() {
                    let _ = connection.sender.send(hint);
                }
                }

                _ => {}
            },

            Message::Response(resp) => {
                if resp.id == lsp_server::RequestId::from(CONFIG_REQ_ID) {
                    if let Some(result) = resp.result {
                        // workspace/configuration returns an array (one entry per item).
                        if let Ok(mut configs) =
                            serde_json::from_value::<Vec<serde_json::Value>>(result)
                        {
                            if let Some(settings) = configs.drain(..).next() {
                                server.apply_config(&settings);
                                // Tell Zed to re-request tokens for all open documents so
                                // that extra keywords take effect immediately without
                                // requiring the user to edit or reopen each file.
                                let _ =
                                    connection.sender.send(Message::Request(lsp_server::Request {
                                        id: lsp_server::RequestId::from(REFRESH_REQ_ID),
                                        method: "workspace/semanticTokens/refresh".to_string(),
                                        params: serde_json::Value::Null,
                                    }));
                            }
                        }
                    }
                }
                // Responses to other server-initiated requests (e.g. REFRESH_REQ_ID) are
                // intentionally ignored — we fire-and-forget those requests.
                if let Some(hint) = server.take_hint_if_needed() {
                    let _ = connection.sender.send(hint);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let (base_keywords, legend) = build_keywords();
    let server = Server::new(base_keywords, &legend);

    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            // INCREMENTAL sync: clients send only the changed ranges, reducing
            // data transfer for large files. `apply_text_change` rebuilds the
            // full text, and the hit cache absorbs repeated token requests for
            // the same document version.
            TextDocumentSyncKind::INCREMENTAL,
        )),
        semantic_tokens_provider: Some(
            SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions::default(),
                legend,
                range: Some(true),
                full: Some(SemanticTokensFullOptions::Bool(true)),
            }),
        ),
        ..Default::default()
    })
    .unwrap();

    connection.initialize(server_capabilities).unwrap();
    run(connection, server);
    io_threads.join().unwrap();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::Position;

    fn make_keywords() -> Vec<Keyword> {
        let (kws, _) = build_keywords();
        kws
    }

    fn make_server() -> Server {
        let (kws, legend) = build_keywords();
        Server::new(kws, &legend)
    }

    fn make_range(start_line: u32, start_col: u32, end_line: u32, end_col: u32) -> Range {
        Range {
            start: Position { line: start_line, character: start_col },
            end: Position { line: end_line, character: end_col },
        }
    }

    // --- scanning -----------------------------------------------------------

    #[test]
    fn test_scan_basic() {
        let kws = make_keywords();
        let tokens = scan_tokens("// TODO: fix this\n// FIXME: urgent", &kws);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 3);
        assert_eq!(tokens[0].length, 4);
        assert_eq!(tokens[1].delta_line, 1);
        assert_eq!(tokens[1].delta_start, 3);
        assert_eq!(tokens[1].length, 5);
    }

    #[test]
    fn test_no_partial_match() {
        let kws = make_keywords();
        assert_eq!(scan_tokens("TODOS and TODOLIST", &kws).len(), 0);
    }

    #[test]
    fn test_all_keywords() {
        let kws = make_keywords();
        let text = "TODO FIXME HACK NOTE INFO WARN WARNING BUG XXX DEPRECATED";
        assert_eq!(scan_tokens(text, &kws).len(), 10);
    }

    #[test]
    fn test_delta_encoding_same_line() {
        let kws = make_keywords();
        let tokens = scan_tokens("TODO and FIXME on same line", &kws);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].delta_start, 0);
        assert_eq!(tokens[1].delta_line, 0);
        assert_eq!(tokens[1].delta_start, 9);
    }

    #[test]
    fn test_empty_document() {
        let kws = make_keywords();
        assert!(scan_tokens("", &kws).is_empty());
        assert!(scan_tokens("no keywords here", &kws).is_empty());
    }

    #[test]
    fn test_utf16_column() {
        let kws = make_keywords();
        let tokens = scan_tokens("// あいう TODO: something", &kws);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].delta_start, 7);
    }

    // --- range --------------------------------------------------------------

    #[test]
    fn range_filters_to_requested_lines() {
        let kws = make_keywords();
        let tokens = scan_tokens_in_range("TODO\nFIXME\nBUG", &kws, &make_range(0, 0, 2, 0));
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].length, 4);
        assert_eq!(tokens[1].length, 5);
    }

    #[test]
    fn range_start_is_inclusive() {
        let kws = make_keywords();
        let tokens = scan_tokens_in_range("prefix TODO suffix", &kws, &make_range(0, 7, 0, 100));
        assert_eq!(tokens.len(), 1);
    }

    #[test]
    fn range_end_is_exclusive() {
        let kws = make_keywords();
        let tokens = scan_tokens_in_range("TODO\nFIXME", &kws, &make_range(0, 0, 1, 0));
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].length, 4);
    }

    #[test]
    fn range_with_no_keywords_returns_empty() {
        let kws = make_keywords();
        let tokens =
            scan_tokens_in_range("TODO\nno keywords here\nFIXME", &kws, &make_range(1, 0, 2, 0));
        assert!(tokens.is_empty());
    }

    #[test]
    fn range_delta_encoding_is_from_document_origin() {
        let kws = make_keywords();
        let tokens = scan_tokens_in_range("\n\n\n\n\nTODO", &kws, &make_range(3, 0, 6, 0));
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].delta_line, 5);
        assert_eq!(tokens[0].delta_start, 0);
    }

    // --- incremental text apply --------------------------------------------

    #[test]
    fn apply_change_replaces_range() {
        // "hello world" → replace "world" (col 6–11) with "Rust"
        let text = "hello world";
        let change = lsp_types::TextDocumentContentChangeEvent {
            range: Some(make_range(0, 6, 0, 11)),
            range_length: None,
            text: "Rust".to_string(),
        };
        assert_eq!(apply_text_change(text, &change), "hello Rust");
    }

    #[test]
    fn apply_change_inserts_newline() {
        let text = "line1\nline2";
        let change = lsp_types::TextDocumentContentChangeEvent {
            range: Some(make_range(0, 5, 0, 5)),
            range_length: None,
            text: "\ninserted".to_string(),
        };
        assert_eq!(apply_text_change(text, &change), "line1\ninserted\nline2");
    }

    #[test]
    fn apply_change_full_replacement_when_no_range() {
        let text = "old content";
        let change = lsp_types::TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "new content".to_string(),
        };
        assert_eq!(apply_text_change(text, &change), "new content");
    }

    #[test]
    fn hit_cache_is_warm_on_second_request() {
        let mut server = make_server();
        let uri: Url = "file:///tmp/test.rs".parse().unwrap();
        server.documents.insert(uri.clone(), Document {
            text: "// TODO: first\n// FIXME: second".to_string(),
            version: 1,
            cached_hits: None,
        });
        // First request scans and warms the cache.
        let t1 = server.tokens_for(&uri);
        assert!(server.documents[&uri].cached_hits.is_some());
        // Second request returns the same result without re-scanning.
        let t2 = server.tokens_for(&uri);
        assert_eq!(t1.data, t2.data);
    }

    #[test]
    fn hit_cache_invalidated_on_text_change() {
        let mut server = make_server();
        let uri: Url = "file:///tmp/test.rs".parse().unwrap();
        server.documents.insert(uri.clone(), Document {
            text: "// TODO: fix".to_string(),
            version: 1,
            cached_hits: None,
        });
        server.tokens_for(&uri); // warm cache
        assert!(server.documents[&uri].cached_hits.is_some());

        // Simulate a didChange that clears the cache.
        server.documents.get_mut(&uri).unwrap().cached_hits = None;
        server.documents.get_mut(&uri).unwrap().text = "// no keywords".to_string();

        let tokens = server.tokens_for(&uri);
        assert!(tokens.data.is_empty());
    }

    // --- configurable keywords ----------------------------------------------

    #[test]
    fn extra_keywords_are_recognized() {
        let mut server = make_server();
        server.apply_config(&serde_json::json!({ "extraKeywords": ["IDEA", "QUESTION"] }));
        let tokens = scan_tokens("IDEA: implement this\nQUESTION: is this right?", &server.keywords);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].token_type, 8);
        assert_eq!(tokens[1].token_type, 8);
    }

    #[test]
    fn extra_keywords_are_case_normalised() {
        let mut server = make_server();
        server.apply_config(&serde_json::json!({ "extraKeywords": ["idea"] }));
        let tokens = scan_tokens("IDEA: implement this", &server.keywords);
        assert_eq!(tokens.len(), 1);
    }

    #[test]
    fn extra_keywords_cleared_on_empty_config() {
        let mut server = make_server();
        server.apply_config(&serde_json::json!({ "extraKeywords": ["IDEA"] }));
        server.apply_config(&serde_json::json!({}));
        assert_eq!(scan_tokens("IDEA: implement this", &server.keywords).len(), 0);
    }

    #[test]
    fn base_keywords_still_work_after_config_change() {
        let mut server = make_server();
        server.apply_config(&serde_json::json!({ "extraKeywords": ["IDEA"] }));
        let tokens = scan_tokens("TODO: still works", &server.keywords);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, 0);
    }

    // --- should_send_setup_hint ---------------------------------------------

    #[test]
    fn setup_hint_triggers_when_eligible_and_no_tokens() {
        let mut server = make_server();
        assert!(!server.should_send_setup_hint());
        server.hint_eligible = true;
        assert!(server.should_send_setup_hint());
    }

    #[test]
    fn setup_hint_not_eligible_until_two_documents_opened() {
        let mut server = make_server();
        server.documents_opened = 1;
        assert!(!server.hint_eligible);
        server.documents_opened = 2;
        server.hint_eligible = true; // mirrors what run() does
        assert!(server.should_send_setup_hint());
    }

    #[test]
    fn setup_hint_suppressed_once_sent() {
        let mut server = make_server();
        server.hint_eligible = true;
        server.setup_hint_sent = true;
        assert!(!server.should_send_setup_hint());
    }

    #[test]
    fn setup_hint_suppressed_when_tokens_ever_requested() {
        let mut server = make_server();
        server.hint_eligible = true;
        server.semantic_tokens_ever_requested = true;
        assert!(!server.should_send_setup_hint());
    }

    // --- Server::tokens_for_range -------------------------------------------

    #[test]
    fn server_tokens_for_range_returns_filtered_hits() {
        let mut server = make_server();
        let uri: Url = "file:///tmp/range_test.rs".parse().unwrap();
        server.documents.insert(uri.clone(), Document {
            text: "TODO\nFIXME\nBUG".to_string(),
            version: 1,
            cached_hits: None,
        });
        let range = make_range(0, 0, 1, 0); // line 0 only
        let tokens = server.tokens_for_range(&uri, &range);
        assert_eq!(tokens.data.len(), 1); // 1 token
        assert_eq!(tokens.data[0].length, 4); // length of "TODO"
        assert!(server.semantic_tokens_ever_requested);
    }

    #[test]
    fn server_tokens_for_range_warms_cache() {
        let mut server = make_server();
        let uri: Url = "file:///tmp/range_cache.rs".parse().unwrap();
        server.documents.insert(uri.clone(), Document {
            text: "TODO FIXME".to_string(),
            version: 1,
            cached_hits: None,
        });
        server.tokens_for_range(&uri, &make_range(0, 0, 0, 5));
        assert!(server.documents[&uri].cached_hits.is_some());
    }

    #[test]
    fn server_tokens_for_unknown_uri_returns_empty() {
        let mut server = make_server();
        let uri: Url = "file:///tmp/ghost.rs".parse().unwrap();
        assert!(server.tokens_for(&uri).data.is_empty());
        assert!(server.tokens_for_range(&uri, &make_range(0, 0, 1, 0)).data.is_empty());
    }

    // --- apply_config cache invalidation ------------------------------------

    #[test]
    fn apply_config_invalidates_cached_hits() {
        let mut server = make_server();
        let uri: Url = "file:///tmp/cache_inv.rs".parse().unwrap();
        server.documents.insert(uri.clone(), Document {
            text: "IDEA: test".to_string(),
            version: 1,
            cached_hits: Some(vec![]),
        });
        server.apply_config(&serde_json::json!({ "extraKeywords": ["IDEA"] }));
        assert!(server.documents[&uri].cached_hits.is_none());
    }

    // --- lsp_pos_to_byte multi-line path ------------------------------------

    #[test]
    fn lsp_pos_to_byte_second_line() {
        let text = "hello\nworld";
        // line 1, col 3 → 'l' in "world" → byte offset 9
        assert_eq!(lsp_pos_to_byte(text, 1, 3), 9);
    }

    #[test]
    fn lsp_pos_to_byte_past_end_of_text() {
        let text = "hello";
        // line beyond the last line → text.len()
        assert_eq!(lsp_pos_to_byte(text, 5, 0), text.len());
    }

    #[test]
    fn lsp_pos_to_byte_col_past_end_returns_text_len() {
        let text = "hi\nthere";
        // col 99 on line 0 — the function walks past the '\n' and through
        // all remaining chars, ultimately returning text.len().
        assert_eq!(lsp_pos_to_byte(text, 0, 99), text.len());
    }

    // --- run() in-process tests ---------------------------------------------

    fn make_in_process_connection() -> (Connection, Connection) {
        use crossbeam_channel::unbounded;
        let (s1, r1) = unbounded::<Message>();
        let (s2, r2) = unbounded::<Message>();
        let server_conn = Connection { sender: s1, receiver: r2 };
        let client_conn = Connection { sender: s2, receiver: r1 };
        (server_conn, client_conn)
    }

    fn send(conn: &Connection, msg: serde_json::Value) {
        let m: Message = serde_json::from_value(msg).unwrap();
        conn.sender.send(m).unwrap();
    }

    fn recv(conn: &Connection) -> serde_json::Value {
        let msg = conn.receiver.recv().unwrap();
        serde_json::to_value(msg).unwrap()
    }

    /// Consumes the `workspace/semanticTokens/refresh` request the server sends
    /// after every `apply_config` call, and asserts its method is correct.
    fn recv_refresh(conn: &Connection) {
        let msg = recv(conn);
        assert_eq!(msg["method"], "workspace/semanticTokens/refresh");
    }

    #[test]
    fn run_responds_to_semantic_tokens_full() {
        let (server_conn, client_conn) = make_in_process_connection();
        let (kws, legend) = build_keywords();
        let server = Server::new(kws, &legend);
        let handle = std::thread::spawn(move || run(server_conn, server));

        // Respond to workspace/configuration pull.
        let config_req = recv(&client_conn);
        assert_eq!(config_req["method"], "workspace/configuration");
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": config_req["id"], "result": [{}]
        }));
        recv_refresh(&client_conn);

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": "file:///tmp/run_test.rs", "languageId": "rust",
                "version": 1, "text": "// TODO: fix\n// FIXME: urgent"
            }}
        }));
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 10, "method": "textDocument/semanticTokens/full",
            "params": { "textDocument": { "uri": "file:///tmp/run_test.rs" } }
        }));

        let resp = recv(&client_conn);
        assert_eq!(resp["id"], 10);
        let data = resp["result"]["data"].as_array().unwrap();
        assert_eq!(data.len(), 10); // 2 tokens × 5 fields

        // Shutdown cleanly.
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null
        }));
        recv(&client_conn); // shutdown response
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "exit", "params": null
        }));
        handle.join().unwrap();
    }

    #[test]
    fn run_responds_to_semantic_tokens_range() {
        let (server_conn, client_conn) = make_in_process_connection();
        let (kws, legend) = build_keywords();
        let server = Server::new(kws, &legend);
        let handle = std::thread::spawn(move || run(server_conn, server));

        let config_req = recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": config_req["id"], "result": [{}]
        }));
        recv_refresh(&client_conn);

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": "file:///tmp/range_run.rs", "languageId": "rust",
                "version": 1, "text": "TODO\nFIXME\nBUG"
            }}
        }));
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 11, "method": "textDocument/semanticTokens/range",
            "params": {
                "textDocument": { "uri": "file:///tmp/range_run.rs" },
                "range": { "start": { "line": 0, "character": 0 },
                           "end":   { "line": 1, "character": 0 } }
            }
        }));

        let resp = recv(&client_conn);
        assert_eq!(resp["id"], 11);
        let data = resp["result"]["data"].as_array().unwrap();
        assert_eq!(data.len(), 5); // 1 token (TODO only, line 0)

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null
        }));
        recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "exit", "params": null
        }));
        handle.join().unwrap();
    }

    #[test]
    fn run_applies_extra_keywords_from_config_response() {
        let (server_conn, client_conn) = make_in_process_connection();
        let (kws, legend) = build_keywords();
        let server = Server::new(kws, &legend);
        let handle = std::thread::spawn(move || run(server_conn, server));

        let config_req = recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": config_req["id"],
            "result": [{ "extraKeywords": ["IDEA"] }]
        }));
        recv_refresh(&client_conn);

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": "file:///tmp/extra_kw.rs", "languageId": "rust",
                "version": 1, "text": "IDEA: something"
            }}
        }));
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 12, "method": "textDocument/semanticTokens/full",
            "params": { "textDocument": { "uri": "file:///tmp/extra_kw.rs" } }
        }));

        let resp = recv(&client_conn);
        assert_eq!(resp["id"], 12);
        let data = resp["result"]["data"].as_array().unwrap();
        assert_eq!(data.len(), 5, "IDEA should be highlighted as an extra keyword");

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null
        }));
        recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "exit", "params": null
        }));
        handle.join().unwrap();
    }

    #[test]
    fn run_did_change_updates_tokens() {
        let (server_conn, client_conn) = make_in_process_connection();
        let (kws, legend) = build_keywords();
        let server = Server::new(kws, &legend);
        let handle = std::thread::spawn(move || run(server_conn, server));

        let config_req = recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": config_req["id"], "result": [{}]
        }));
        recv_refresh(&client_conn);

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": "file:///tmp/change.rs", "languageId": "rust",
                "version": 1, "text": "no keywords here"
            }}
        }));
        // Mutate the document to add a keyword.
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": "file:///tmp/change.rs", "version": 2 },
                "contentChanges": [{ "text": "TODO: added" }]
            }
        }));
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 13, "method": "textDocument/semanticTokens/full",
            "params": { "textDocument": { "uri": "file:///tmp/change.rs" } }
        }));

        let resp = recv(&client_conn);
        assert_eq!(resp["id"], 13);
        let data = resp["result"]["data"].as_array().unwrap();
        assert_eq!(data.len(), 5); // TODO token

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null
        }));
        recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "exit", "params": null
        }));
        handle.join().unwrap();
    }

    #[test]
    fn run_did_close_removes_document() {
        let (server_conn, client_conn) = make_in_process_connection();
        let (kws, legend) = build_keywords();
        let server = Server::new(kws, &legend);
        let handle = std::thread::spawn(move || run(server_conn, server));

        let config_req = recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": config_req["id"], "result": [{}]
        }));
        recv_refresh(&client_conn);

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": "file:///tmp/close_me.rs", "languageId": "rust",
                "version": 1, "text": "TODO"
            }}
        }));
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didClose",
            "params": { "textDocument": { "uri": "file:///tmp/close_me.rs" } }
        }));
        // After close, tokens/full should return empty (doc gone).
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 14, "method": "textDocument/semanticTokens/full",
            "params": { "textDocument": { "uri": "file:///tmp/close_me.rs" } }
        }));

        let resp = recv(&client_conn);
        assert_eq!(resp["id"], 14);
        let data = resp["result"]["data"].as_array().unwrap();
        assert!(data.is_empty());

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null
        }));
        recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "exit", "params": null
        }));
        handle.join().unwrap();
    }

    #[test]
    fn run_setup_hint_sent_after_two_opens_without_token_request() {
        let (server_conn, client_conn) = make_in_process_connection();
        let (kws, legend) = build_keywords();
        let server = Server::new(kws, &legend);
        let handle = std::thread::spawn(move || run(server_conn, server));

        let config_req = recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": config_req["id"], "result": [{}]
        }));
        recv_refresh(&client_conn);

        for i in 0..2u32 {
            send(&client_conn, serde_json::json!({
                "jsonrpc": "2.0", "method": "textDocument/didOpen",
                "params": { "textDocument": {
                    "uri": format!("file:///tmp/hint_{i}.rs"), "languageId": "rust",
                    "version": 1, "text": "no keywords"
                }}
            }));
        }

        // The hint fires on the first non-didOpen message, not on didOpen itself.
        // This matches Zed's real behaviour: Zed sends all didOpen notifications as
        // a batch and then sends semanticTokens/full requests. By deferring the check
        // we allow those token requests to cancel the hint before it fires.
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": "file:///tmp/hint_0.rs", "version": 2 },
                "contentChanges": [{ "text": "still no keywords" }]
            }
        }));

        // The server should emit a window/showMessage hint.
        let notif = recv(&client_conn);
        assert_eq!(notif["method"], "window/showMessage");
        assert!(notif["params"]["message"]
            .as_str()
            .unwrap()
            .contains("TODO Highlighter"));

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null
        }));
        recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "exit", "params": null
        }));
        handle.join().unwrap();
    }

    #[test]
    fn run_did_change_configuration_re_pulls_config() {
        let (server_conn, client_conn) = make_in_process_connection();
        let (kws, legend) = build_keywords();
        let server = Server::new(kws, &legend);
        let handle = std::thread::spawn(move || run(server_conn, server));

        // Initial config pull — no extra keywords.
        let config_req = recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": config_req["id"], "result": [{}]
        }));
        recv_refresh(&client_conn);

        // Trigger a settings change; server should re-pull.
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "workspace/didChangeConfiguration",
            "params": { "settings": null }
        }));

        let config_req2 = recv(&client_conn);
        assert_eq!(config_req2["method"], "workspace/configuration");
        // Respond with IDEA as a new extra keyword.
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": config_req2["id"],
            "result": [{ "extraKeywords": ["IDEA"] }]
        }));
        recv_refresh(&client_conn);

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": "file:///tmp/recfg.rs", "languageId": "rust",
                "version": 1, "text": "IDEA: test"
            }}
        }));
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 15, "method": "textDocument/semanticTokens/full",
            "params": { "textDocument": { "uri": "file:///tmp/recfg.rs" } }
        }));

        let resp = recv(&client_conn);
        assert_eq!(resp["id"], 15);
        let data = resp["result"]["data"].as_array().unwrap();
        assert_eq!(data.len(), 5, "IDEA should be highlighted after config re-pull");

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null
        }));
        recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "exit", "params": null
        }));
        handle.join().unwrap();
    }

    /// Regression test for the race condition where a document is already open and
    /// has tokens cached (with only base keywords) before the config response arrives.
    /// After apply_config the server must send `workspace/semanticTokens/refresh` so
    /// that the editor re-requests tokens and picks up the new extra keywords.
    #[test]
    fn run_extra_keywords_refresh_already_open_document() {
        let (server_conn, client_conn) = make_in_process_connection();
        let (kws, legend) = build_keywords();
        let server = Server::new(kws, &legend);
        let handle = std::thread::spawn(move || run(server_conn, server));

        // Initial config pull — no extra keywords yet.
        let config_req = recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": config_req["id"], "result": [{}]
        }));
        recv_refresh(&client_conn);

        // Open a document and request tokens — cached with base keywords only.
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": "file:///tmp/refresh_open.rs", "languageId": "rust",
                "version": 1, "text": "IDEA: check this out"
            }}
        }));
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 20, "method": "textDocument/semanticTokens/full",
            "params": { "textDocument": { "uri": "file:///tmp/refresh_open.rs" } }
        }));
        let resp = recv(&client_conn);
        assert_eq!(resp["id"], 20);
        assert!(
            resp["result"]["data"].as_array().unwrap().is_empty(),
            "IDEA must not be highlighted before extraKeywords is configured"
        );

        // User adds IDEA to extraKeywords in settings.
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "workspace/didChangeConfiguration",
            "params": { "settings": null }
        }));
        let config_req2 = recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": config_req2["id"],
            "result": [{ "extraKeywords": ["IDEA"] }]
        }));

        // Server must send refresh so the editor re-requests tokens for open documents.
        let refresh = recv(&client_conn);
        assert_eq!(
            refresh["method"], "workspace/semanticTokens/refresh",
            "server must send semanticTokens/refresh after applying config"
        );

        // Re-request tokens — cache was invalidated; fresh scan picks up IDEA.
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 21, "method": "textDocument/semanticTokens/full",
            "params": { "textDocument": { "uri": "file:///tmp/refresh_open.rs" } }
        }));
        let resp2 = recv(&client_conn);
        assert_eq!(resp2["id"], 21);
        assert_eq!(
            resp2["result"]["data"].as_array().unwrap().len(), 5,
            "IDEA must be highlighted after extraKeywords config update"
        );

        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null
        }));
        recv(&client_conn);
        send(&client_conn, serde_json::json!({
            "jsonrpc": "2.0", "method": "exit", "params": null
        }));
        handle.join().unwrap();
    }
}
