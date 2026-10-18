# Server startup

When Zed starts the language server, the server advertises what it supports and writes a version line to Zed's LSP log (`dev: open language server logs`). That log line is what users check when troubleshooting.

## Sub-features

- `start-legend` defines one token type (`keyword`) and 10 keyword modifiers.
- `start-caps` advertises incremental text sync (`2`) and both full and range semantic tokens.
- `start-log` sends a `window/logMessage` with `todo-highlight-lsp v<version>: initialized; client semantic-token support: <bool>`.
- `start-refresh` sends a `workspace/semanticTokens/refresh` request right after initialize, so files that were already open get highlighted.

## How to get to it (user POV)

- Open a supported file in Zed (the server starts), then check Zed's LSP log for `todo-highlight-lsp`.

## Driving it with lsp-drive.py

Preconditions:

- Doctor is OK. Doctor itself checks that the logged version matches `lsp/Cargo.toml`.

- **Startup.** Run `$H run startup`. Expect `[PASS] startup`. `record.init_result.capabilities` shows `textDocumentSync: 2` and `semanticTokensProvider.range: true`, and `record.log_messages[0]` starts with `todo-highlight-lsp v`.
- **Refresh.** In `startup/transcript.jsonl`, a server→client frame with `"method": "workspace/semanticTokens/refresh"` follows the initialize response, and the harness answers it.
- **Proof.** Cite `startup/result.json` and the transcript.

## Gotchas

- After a version bump, doctor fails until you rebuild. The four version locations listed in `CLAUDE.md` must agree. This harness only sees `lsp/Cargo.toml` and the binary.
