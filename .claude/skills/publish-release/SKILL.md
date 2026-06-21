---
name: publish-release
description: Publish a new versioned release of todo-highlight to GitHub Releases
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

## Step 4 — Open a PR for the version bump

`main` is protected by a repository ruleset that blocks direct pushes (PR required,
no bypass). The version bump must land via PR like any other change.

```bash
git checkout -b chore/bump-version-x.y.z
git add extension/src/lib.rs extension/extension.toml extension/Cargo.toml lsp/Cargo.toml
git commit -m "chore: bump version to x.y.z"
git push -u origin chore/bump-version-x.y.z
gh pr create --title "chore: bump version to x.y.z" --body "Bump version to x.y.z."
```

Wait for the PR to be reviewed and merged before continuing.

## Step 5 — Tag the merged commit

The ruleset only covers the `main` branch ref, not tags, so this push goes directly.
Tag the merge commit on `main` (not the PR branch) so the tag matches what's on the
default branch.

```bash
git checkout main
git pull --ff-only
git tag vx.y.z
git push origin vx.y.z
```

Pushing the tag triggers `.github/workflows/release.yml`, which cross-compiles
`todo-highlight-lsp` for every target, packages each binary as a `.tar.gz` (macOS/Linux)
or `.zip` (Windows), generates SHA-256 checksums, and creates the GitHub Release with all
assets attached automatically.

## Step 6 — Wait for CI and verify the release

```bash
gh run list --workflow=release.yml --limit 1
# once that run shows "completed / success":
gh release view vx.y.z
```

Confirm all expected assets (and matching `.sha256` files) are present:
- `todo-highlight-lsp-{aarch64,x86_64}-apple-darwin.tar.gz`
- `todo-highlight-lsp-{aarch64,x86_64}-unknown-linux-musl.tar.gz`
- `todo-highlight-lsp-{aarch64,x86_64}-pc-windows-msvc.zip`

## Step 7 — Verify the download works

Install the released version as a dev extension in Zed and confirm the LSP starts. The extension will attempt to download the binary from the new release URL. Check Zed's extension logs for errors.

Expected URL pattern:
```
https://github.com/shionit/zed-todo-highlight/releases/download/vx.y.z/todo-highlight-lsp-{target}.tar.gz
https://github.com/shionit/zed-todo-highlight/releases/download/vx.y.z/todo-highlight-lsp-{target}.zip  (Windows)
```
