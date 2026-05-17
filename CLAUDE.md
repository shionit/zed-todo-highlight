# CLAUDE.md — todo-highlighter

## Verify before marking work done

Run `./check.sh` — it tests, lints, and validates the WASM build in one shot.

## Test coverage requirement

Line coverage for `todo-highlighter-lsp` must stay at or above **80%**.

```bash
# Check coverage (fails if below 80%)
cargo llvm-cov --package todo-highlighter-lsp --fail-under-lines 80
```

`check.sh` runs this automatically — do not mark a task complete if it fails.

## Build commands

`cargo` must be on your PATH. Run `source ~/.cargo/env` or add `~/.cargo/bin` to your
shell profile if needed.

```bash
# LSP server (native binary)
cargo build --release -p todo-highlighter-lsp

# Extension — always use this target flag; plain cargo check gives false confidence
cargo check -p todo-highlighter-extension --target wasm32-wasip1

# Full test suite
cargo test --workspace

# Lint — warnings are treated as errors
cargo clippy -p todo-highlighter-lsp -- -D warnings
```

## Dev install in Zed

1. `cargo build --release -p todo-highlighter-lsp`
2. `cp target/release/todo-highlighter-lsp ~/.local/bin/`
3. Zed → Extensions → Install Dev Extension → select the **`extension/`** subdirectory
4. Add to Zed `settings.json`: `"semantic_tokens": "combined"`

Note: select `extension/` (not the repo root). Zed expects a crate-level `Cargo.toml`
([package] + cdylib) alongside `extension.toml`, and the repo-root `Cargo.toml` is a
workspace manifest which Zed rejects.

## Non-obvious constraints

- **WASM target is mandatory**: The extension crate only compiles correctly under
  `--target wasm32-wasip1`. Omitting it succeeds silently but produces a wrong build.
- **Exact version pins**: `lsp-server` and `url` are pinned with `=`. Do not widen
  them to range versions — see the comment in each `Cargo.toml` for the reason.
- **LSP_VERSION must stay in sync**: When bumping the version, update all four
  places together: `LSP_VERSION` in `extension/src/lib.rs`, `version` in
  `extension/extension.toml`, `version` in `extension/Cargo.toml`, and `version` in
  `lsp/Cargo.toml`. Mismatch causes the extension to download the wrong binary.

## What NOT to do

- Do NOT use `tower-lsp` — it brings `tokio`, making the binary significantly heavier.
- Do NOT use TreeSitter injection or `highlights.scm` for keyword highlighting —
  they cannot scan for arbitrary keywords dynamically.
- Do NOT add `async`/`tokio` to the LSP server.
- Do NOT widen exact dependency pins without updating the rationale comment.

## Skills

Use these for multi-step tasks to avoid missing steps:

- `/add-keyword` — adds a new highlight keyword end-to-end
- `/publish-release` — publishes a versioned release to GitHub Releases
