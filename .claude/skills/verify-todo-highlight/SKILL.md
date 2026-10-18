---
name: verify-todo-highlight
description: Drive the real todo-highlight-lsp binary over stdio exactly as Zed does (initialize, didOpen/didChange/didClose, semanticTokens/full+range) and capture JSON-RPC transcripts as proof. Use to confirm a change to keyword highlighting, token encoding, incremental sync, or the setup-hint notification actually works end to end, beyond unit tests. Also covers what can and cannot be verified in the Zed GUI.
---

# Verify todo-highlight

## What the user touches, and what you can drive

- **User surface:** the Zed editor. They open a file and see `TODO`, `FIXME`, and so on highlighted, or an info toast telling them to enable `"semantic_tokens": "combined"`. The Zed GUI is the user's own running app with their settings. **Do not drive it.** There is no isolated profile, and opening or closing files there corrupts their session.
- **Drivable surface:** `todo-highlight-lsp`, a stdio LSP server. Zed only renders what this server sends back, so every user-visible behavior except theme color and toast styling is decided by the JSON-RPC messages it exchanges. The harness speaks that protocol to a freshly built binary.
- **Not drivable outside Zed:** the WASM extension in `extension/src/lib.rs` (resolving the binary on PATH or downloading it from GitHub Releases). It only runs inside Zed's extension host. For that crate, `cargo check -p todo-highlight-extension --target wasm32-wasip1` (part of `./check.sh`) is the ceiling here. Say so in your report rather than implying it was exercised.

## Launch

No long-running server is involved. Build once, then each drive spawns its own child process:

```bash
source ~/.cargo/env 2>/dev/null; cargo build --release -p todo-highlight-lsp
```

Ready = `target/release/todo-highlight-lsp` exists and doctor says `doctor: OK`. Teardown is automatic: the harness sends `shutdown` + `exit`, then kills its own child (by handle, never by name) if the process is still alive.

Isolation: there are no ports and no data dirs. Every run is a separate process with its own stdio, so parallel runs are safe and cannot touch the copy Zed is running from `~/.local/bin`.

## Doctor

```bash
.claude/skills/verify-todo-highlight/lsp-drive.py doctor
```

Read-only check. It fails (exit 1) if the binary is missing, if the binary is **older than anything in `lsp/src`** (you'd be verifying stale code), or if the version the server logs at startup differs from `lsp/Cargo.toml`. It also prints the legend and the PATH copy that a real Zed would launch. Run it first, and again whenever a result looks wrong.

## Drive

```bash
H=.claude/skills/verify-todo-highlight/lsp-drive.py
$H list                 # scenario names
$H run highlight        # one scenario
$H run all              # every scenario in one evidence dir
TODO_LSP_BIN=/path/to/other/build $H run all   # drive a different binary
```

Exit codes: `0` all checks passed, `1` a check failed, `2` harness/usage error. Each scenario prints `[PASS]`/`[FAIL]` plus one line per check.

The client capabilities mirror Zed's. When Zed has `"semantic_tokens"` enabled, it advertises `capabilities.textDocument.semanticTokens`. When the setting is off, Zed still advertises it but never sends token requests. A Zed version too old for semantic tokens advertises nothing (the `hint-unsupported` scenario). The harness answers the server's `workspace/semanticTokens/refresh` request with `null` the way Zed does.

To add a scenario, write an `sc_<name>(session)` function in `lsp-drive.py` that returns `(record, [(check_name, bool), ...])` and register it in `SCENARIOS`. Use `open_doc`, `full`, `session.request`, `session.notify`, `session.settle`, and `session.decode`. Assert on decoded tokens (`line`, `col`, `len`, `type`, `modifiers`), not on raw integers.

Feature recipes live in [`features/README.md`](features/README.md). Read the index before claiming a feature is verified.

## Evidence

Every run writes `target/verify-evidence/<YYYYMMDD-HHMMSS-pid>/<scenario>/`:

- `transcript.jsonl`: every JSON-RPC frame in both directions, timestamped. This is the action and the resulting state.
- `result.json`: decoded tokens with the matched source text, `window/showMessage` texts, each check's outcome, and the server's exit code.
- `stderr.txt`: server stderr (`bad didOpen params` and similar show up here).

Doctor writes to `target/verify-evidence/doctor/`. Use `--out DIR` to put a run elsewhere.

Proof standards:
- Drive the real binary through the protocol. Never call Rust internals or add test-only hooks. `cargo test` already covers internals.
- Cite the evidence dir and the decoded tokens or message text in your report, not just "PASS".
- Prove a regression fix with a negative control when you can: run the scenario against a build without the fix (`CARGO_TARGET_DIR=<scratch> cargo build ...` on the old code, then `TODO_LSP_BIN=...`) and show that it FAILs.
- The harness proves what Zed receives. It does not prove pixels. If the change is about how Zed renders tokens (theme `keyword` style, `global_lsp_settings.semantic_token_rules`), list it as **unverified in GUI** and ask the user to look in their Zed.

## Cleanup

Nothing to tear down: each child exits or is killed by the harness before `run` returns. Confirm with `pgrep -fl todo-highlight-lsp`. Anything listed there that you did not start belongs to the user's Zed. Leave it alone. Evidence in `target/verify-evidence/` survives cleanup, but `cargo clean` deletes it. Copy anything you need to keep before running that.

## Helpers

- `lsp-drive.py`: executable, stdlib-only Python 3.9+. Subcommands: `doctor`, `list`, `run <scenario|all> [--out DIR]`. Env: `TODO_LSP_BIN` overrides the binary (default `target/release/todo-highlight-lsp`).
