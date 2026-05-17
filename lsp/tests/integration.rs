/// End-to-end LSP protocol round-trip test.
///
/// Spawns the compiled `todo-highlighter-lsp` binary, performs a full
/// LSP handshake, and asserts that semantic tokens come back correctly.
/// Cargo builds the binary before running integration tests and exposes
/// its path via `CARGO_BIN_EXE_todo-highlighter-lsp`.
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Sends a single LSP message (Content-Length framing).
fn lsp_send(writer: &mut impl Write, msg: &serde_json::Value) {
    let body = msg.to_string();
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
    writer.flush().unwrap();
}

/// Reads one LSP message and returns it as JSON.
fn lsp_recv(reader: &mut BufReader<impl Read>) -> serde_json::Value {
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(s) = trimmed.strip_prefix("Content-Length: ") {
            content_length = s.trim().parse().unwrap_or(0);
        }
    }
    assert!(content_length > 0, "no Content-Length header received from server");
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).expect("server returned invalid JSON")
}

/// RAII guard: kills and reaps the child process on drop so the OS doesn't
/// accumulate zombie processes when a test assertion fails.
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_server() -> ChildGuard {
    let binary = env!("CARGO_BIN_EXE_todo-highlighter-lsp");
    let child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn todo-highlighter-lsp");
    ChildGuard(child)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn semantic_tokens_full_round_trip() {
    let mut guard = spawn_server();
    let child = &mut guard.0;
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    // ── initialize ──────────────────────────────────────────────────────────
    lsp_send(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "capabilities": {}, "rootUri": null }
        }),
    );
    let resp = lsp_recv(&mut stdout);
    assert_eq!(resp["id"], 1, "initialize response id mismatch");
    assert!(
        resp["result"]["capabilities"]["semanticTokensProvider"].is_object(),
        "server must advertise semanticTokensProvider"
    );

    // ── initialized ─────────────────────────────────────────────────────────
    lsp_send(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );

    // ── textDocument/didOpen ─────────────────────────────────────────────────
    lsp_send(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///tmp/test.rs",
                    "languageId": "rust",
                    "version": 1,
                    "text": "// TODO: fix this\n// FIXME: urgent"
                }
            }
        }),
    );

    // ── textDocument/semanticTokens/full ─────────────────────────────────────
    lsp_send(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/semanticTokens/full",
            "params": { "textDocument": { "uri": "file:///tmp/test.rs" } }
        }),
    );
    let resp = lsp_recv(&mut stdout);
    assert_eq!(resp["id"], 2, "semanticTokens/full response id mismatch");

    // Token data is a flat array: [delta_line, delta_start, length, token_type, modifiers, ...]
    // Token 0: TODO  — line 0, col 3, len 4, type 0 (todoKeyword),  mods 0
    // Token 1: FIXME — delta_line 1, col 3, len 5, type 1 (fixmeKeyword), mods 0
    let data = resp["result"]["data"].as_array().expect("expected data array");
    assert_eq!(data.len(), 10, "expected 2 tokens × 5 fields");
    assert_eq!(data[0], 0, "TODO delta_line");
    assert_eq!(data[1], 3, "TODO delta_start");
    assert_eq!(data[2], 4, "TODO length");
    assert_eq!(data[3], 0, "TODO token_type (todoKeyword)");
    assert_eq!(data[4], 0, "TODO modifiers");
    assert_eq!(data[5], 1, "FIXME delta_line");
    assert_eq!(data[6], 3, "FIXME delta_start");
    assert_eq!(data[7], 5, "FIXME length");
    assert_eq!(data[8], 1, "FIXME token_type (fixmeKeyword)");
    assert_eq!(data[9], 0, "FIXME modifiers");

    // ── shutdown / exit ──────────────────────────────────────────────────────
    lsp_send(
        &mut stdin,
        &serde_json::json!({ "jsonrpc": "2.0", "id": 3, "method": "shutdown", "params": null }),
    );
    let resp = lsp_recv(&mut stdout);
    assert_eq!(resp["id"], 3);
    assert!(resp["error"].is_null(), "shutdown should not return an error");

    lsp_send(
        &mut stdin,
        &serde_json::json!({ "jsonrpc": "2.0", "method": "exit", "params": null }),
    );
}

#[test]
fn unknown_method_returns_method_not_found() {
    let mut guard = spawn_server();
    let child = &mut guard.0;
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    lsp_send(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "capabilities": {}, "rootUri": null }
        }),
    );
    lsp_recv(&mut stdout); // consume initialize response

    lsp_send(
        &mut stdin,
        &serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    );

    lsp_send(
        &mut stdin,
        &serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "nonExistent/method", "params": {} }),
    );
    let resp = lsp_recv(&mut stdout);
    assert_eq!(resp["id"], 2);
    assert!(resp["error"].is_object(), "unknown method must return an error");
    // lsp-server ErrorCode::MethodNotFound = -32601
    assert_eq!(resp["error"]["code"], -32601);
}
