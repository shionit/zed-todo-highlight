---
name: publish-release
description: Publish a new versioned release of todo-highlighter to GitHub Releases
---

Publish version: $ARGUMENTS

Complete every step in order. A missed step breaks the binary download for all users.

## Step 1 — Decide the new version

Follow semver:
- Patch (0.1.x): bug fixes, no new keywords or API changes
- Minor (0.x.0): new keywords or new LSP features
- Major (x.0.0): breaking changes to settings schema or LSP protocol

## Step 2 — Bump version in all four places

Update these files to the new version string (e.g. `0.2.0`). All four must match.

| File | Field |
|---|---|
| `extension/src/lib.rs` | `const LSP_VERSION: &str = "x.y.z";` |
| `extension/extension.toml` | `version = "x.y.z"` |
| `extension/Cargo.toml` | `version = "x.y.z"` |
| `lsp/Cargo.toml` | `version = "x.y.z"` |

## Step 3 — Run full verification

```bash
./check.sh
```

Do not proceed if any check fails.

## Step 4 — Commit the version bump

```bash
git add extension/src/lib.rs extension.toml extension/Cargo.toml lsp/Cargo.toml
git commit -m "chore: bump version to x.y.z"
git tag vx.y.z
git push origin main --tags
```

## Step 5 — Build release binaries for all targets

Run each cross-compilation. Requires the targets to be installed via `rustup target add`.

```bash
CARGO=~/.cargo/bin/cargo

# macOS Apple Silicon
$CARGO build --release -p todo-highlighter-lsp --target aarch64-apple-darwin

# macOS Intel
$CARGO build --release -p todo-highlighter-lsp --target x86_64-apple-darwin

# Linux ARM64
$CARGO build --release -p todo-highlighter-lsp --target aarch64-unknown-linux-gnu

# Linux x86_64
$CARGO build --release -p todo-highlighter-lsp --target x86_64-unknown-linux-gnu
```

Copy and rename binaries to match the names the extension downloads:
```bash
cp target/aarch64-apple-darwin/release/todo-highlighter-lsp   dist/todo-highlighter-lsp-aarch64-apple-darwin
cp target/x86_64-apple-darwin/release/todo-highlighter-lsp    dist/todo-highlighter-lsp-x86_64-apple-darwin
cp target/aarch64-unknown-linux-gnu/release/todo-highlighter-lsp  dist/todo-highlighter-lsp-aarch64-unknown-linux-gnu
cp target/x86_64-unknown-linux-gnu/release/todo-highlighter-lsp   dist/todo-highlighter-lsp-x86_64-unknown-linux-gnu
```

## Step 6 — Create GitHub Release and upload assets

```bash
gh release create vx.y.z \
  --title "v x.y.z" \
  --notes "Release notes here" \
  dist/todo-highlighter-lsp-aarch64-apple-darwin \
  dist/todo-highlighter-lsp-x86_64-apple-darwin \
  dist/todo-highlighter-lsp-aarch64-unknown-linux-gnu \
  dist/todo-highlighter-lsp-x86_64-unknown-linux-gnu
```

## Step 7 — Verify the download works

Install the released version as a dev extension in Zed and confirm the LSP starts. The extension will attempt to download the binary from the new release URL. Check Zed's extension logs for errors.

Expected URL pattern:
```
https://github.com/shionit/todo-highlighter/releases/download/vx.y.z/todo-highlighter-lsp-{target}
```
