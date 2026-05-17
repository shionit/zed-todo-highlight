# TODO — todo-highlighter

## Project Status

The extension is **working end-to-end** — installed as a dev extension in Zed with keywords highlighting correctly in the editor:
- WASM extension (`extension/src/lib.rs`) — registers LSP server, downloads pre-built binaries for macOS, Linux, and Windows
- LSP server (`lsp/src/main.rs`) — scans buffers, emits semantic tokens, setup notification
- 22 tests (20 unit + 2 integration, full LSP protocol round-trip)
- CI workflow (test + clippy + WASM check) and release workflow (6-platform binary build + GitHub Release)
- `extension/extension.toml` co-located with the extension crate — Zed requires the manifest next to the `[package]` Cargo.toml

---

## High Priority

- [x] **Verify WASM sandbox binary spawning**
  - `worktree.which()` is confirmed correct — Zed proxies it through the host runtime, WASM sandbox does not block it
  - Added `cached_binary_path: Option<String>` to extension struct (canonical pattern; avoids re-search per worktree)
  - Added stale-cache check: re-searches if binary no longer exists on disk
  - Added actionable error message with build/install instructions
  - Extension compiles clean for `wasm32-wasip1` target

- [x] **Add binary download/bundle mechanism in extension**
  - Implemented 3-step resolution: cache → PATH → GitHub Releases download
  - `download_binary()` builds a platform-specific URL from `zed::current_platform()`
  - Supported platforms: macOS (aarch64, x86_64), Linux (aarch64, x86_64), Windows (x86_64, aarch64)
  - Windows binaries use `.exe` suffix in both the download URL and the on-disk cache path
  - Versioned on-disk path (`bin/todo-highlighter-lsp-{target}-v{VERSION}`) prevents stale cached binaries after upgrades
  - `zed::make_file_executable` is called on all platforms (no-op on Windows)
  - Compiles clean for `wasm32-wasip1` with zero clippy warnings

- [x] **Validate range-based semantic tokens**
  - Fixed: range handler was returning all document tokens, ignoring `params.range`
  - Refactored `scan_tokens` into `find_hits` (raw sorted positions) + `delta_encode` (shared encoder)
  - Added `scan_tokens_in_range`: filters hits to `[range.start, range.end)` then delta-encodes from doc origin
  - Added `Server::tokens_for_range` method wired into the `SemanticTokensRangeRequest` handler
  - Added 5 range-specific tests: line filtering, inclusive start, exclusive end, empty result, delta encoding from doc origin

---

## Medium Priority

- [x] **Integration test: LSP protocol round-trip**
  - `lsp/tests/integration.rs` spawns the compiled binary and drives it over real LSP stdio
  - `semantic_tokens_full_round_trip`: full handshake → `didOpen` → `semanticTokens/full`, asserts all 10 token data fields
  - `unknown_method_returns_method_not_found`: asserts error code `-32601` for unknown requests
  - `ChildGuard` RAII struct kills and reaps the process on drop — no zombie leaks on test failure

- [ ] **User-configurable keywords via LSP workspace config** *(postponed — not yet working correctly)*
  - Handles `workspace/didChangeConfiguration` notification
  - Users add extra keywords in Zed settings: `{ "extraKeywords": ["IDEA", "QUESTION"] }`
  - Extra keywords normalise to uppercase and reuse the `xxxKeyword` token type (purple)
  - `Server::apply_config` rebuilds the active keyword set; base keywords always preserved
  - 4 unit tests: recognition, case normalisation, clearing, base keywords survive config change

- [x] **CI/CD: GitHub Actions workflow**
  - `.github/workflows/ci.yml`: test + clippy + WASM check on every push/PR
  - `.github/workflows/release.yml`: triggers on `v*` tags; builds for all 6 targets
    (macOS arm64/x86_64, Linux x86_64 native, Linux arm64 via `cross`, Windows x86_64/arm64)
  - Windows builds use `windows-latest` runner with MSVC toolchain; `.exe` suffix handled via `bin_suffix` matrix field
  - Generates `.sha256` checksums for each binary and attaches all 12 assets to the GitHub Release

- [x] **Error handling improvements in extension/src/lib.rs**
  - `eprintln!` logging at each resolution step: cache miss, PATH hit, download start, download success
  - Zed routes extension stderr to its log viewer — users can diagnose startup failures without rebuilding
  - Unsupported platform error includes build-from-source instructions
  - Integrity check failure removes the corrupted binary so it won't be served from cache

---

## Low Priority

- [x] **Performance: incremental scanning for large files**
  - Switched from `FULL` to `INCREMENTAL` text sync — clients send only changed ranges, reducing data transfer
  - Added `lsp_pos_to_byte` + `apply_text_change` to apply incremental change events to the stored text
  - Added `Document` struct with `cached_hits: Option<Vec<Hit>>` — scan results cached per document version
  - `tokens_for` and `tokens_for_range` populate the cache on first access; subsequent requests return immediately
  - Cache invalidated on any `didChange` (text changed) or `apply_config` (keyword set changed)
  - 5 new tests: range replacement, newline insertion, full replacement, cache warm, cache invalidation
  - 22/22 tests passing (20 unit + 2 integration)

- [x] **Refactor: extract keyword definitions to separate module**
  - `Keyword` struct and `build_keywords()` extracted to `lsp/src/keywords.rs`
  - `scan_tokens` and `scan_tokens_in_range` marked `#[cfg(test)]` — server now uses `find_hits` + `delta_encode` directly
  - `main.rs` reduced from ~370 to ~370 lines but cleanly separated by concern via `mod keywords`

- [x] **README improvements**
  - Single copy-paste settings block covering both required settings (`semantic_tokens` + `semantic_token_rules`)
  - Added **Adding Extra Keywords** section with `extraKeywords` config example *(postponed — not yet working correctly)*
  - Added **Troubleshooting** section covering: tokens not appearing, partial matches, color conflicts
  - Added Windows dev install instructions alongside macOS/Linux
  - Added callout explaining why both settings are needed (Zed architectural limitation)

- [x] **Setup notification when semantic tokens are not active**
  - LSP server tracks whether Zed has ever sent a `semanticTokens/full` or `semanticTokens/range` request
  - After the 2nd `didOpen` with no semantic token request, sends a `window/showMessage` info notification
  - Notification explains that both required settings are missing and points to the README
  - Users who have already configured the extension never see it (requests arrive before the 2nd document)

- [x] **Background color for highlighted keywords**
  - `background_color` is a native field in Zed's `semantic_token_rules` — no LSP server changes required
  - Dark-mode palette: foreground hue at low lightness (~10–15% luminance), consistent across all 10 keywords
  - Updated README: installation example now includes `background_color` for all keywords
  - Expanded "Customizing Colors" → "Background color" subsection with full palette table and light-theme guidance
  - Updated `window/showMessage` setup hint to mention background colors are available in the README

- [ ] **Publish to Zed extension registry**
  - Verify extension.toml is complete and valid
  - Submit PR to zed-industries/extensions

---

## Open Risks

| Risk | Status |
|------|--------|
| WASM sandbox restricts child-process spawning | **Resolved** — `worktree.which()` confirmed working |
| Token type names must match exactly in `semantic_token_rules` | **Verified** — names match between LSP and JSON config |
| Range tokens must be sorted and non-overlapping | **Resolved** — `scan_tokens_in_range` filters + re-encodes |
| Binary download integrity | **Mitigated** — HTTPS transport + CI generates `.sha256` assets on release |
| Dev extension install fails | **Resolved** — two root causes fixed: (1) add `~/.cargo/bin` to `/etc/paths.d/rust` so Zed finds `rustc`; (2) select `extension/` subdirectory (not repo root) since Zed rejects workspace `Cargo.toml` |
