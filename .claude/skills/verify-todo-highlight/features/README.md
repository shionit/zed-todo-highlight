# todo-highlight verification map

This directory is the maintained source for verifying the user-facing behavior of TODO Highlight. Read this index before driving anything, then use the matching feature file as the recipe.

## Baseline preconditions

- `cargo build --release -p todo-highlight-lsp` has run on the code under test.
- `.claude/skills/verify-todo-highlight/lsp-drive.py doctor` prints `doctor: OK`. That means the binary exists, is newer than `lsp/src`, and reports the `lsp/Cargo.toml` version.
- Never drive the user's running Zed or the `todo-highlight-lsp` processes it owns.

## Driving conventions

- `H=.claude/skills/verify-todo-highlight/lsp-drive.py`. Every recipe is `$H run <scenario>`.
- Each scenario starts a fresh server process, so there is no shared state between recipes.
- Assert on decoded tokens (`line`, `col`, `len`, `type`, `modifiers`, and matched `text` in `result.json`), never on raw delta integers.
- Positions are LSP UTF-16 code units, the same as Zed uses.

## Proof and skip reporting

- Cite the evidence dir that `run` printed. It contains `transcript.jsonl` (action + response), `result.json` (decoded state + checks), and `stderr.txt`.
- For a behavior change, show both sides: the new run PASSes, and when practical the old build FAILs the same scenario.
- Anything rendered by Zed itself (colors, toast appearance, extension download) is outside the harness. Report it as unverified, not as verified through the LSP path.

## Feature entry contract

Each feature file has an H1 title, one paragraph describing the user-visible behavior, then exactly four H2s in order: `Sub-features`, `How to get to it (user POV)`, `Driving it with lsp-drive.py` (starting with `Preconditions:`), and `Gotchas`.

## Features

- [Keyword highlighting](./keyword-highlighting.md): all keywords in an open file come back as `keyword` tokens with per-keyword modifiers, word boundaries, and UTF-16 columns.
- [Live editing](./live-editing.md): typing or deleting a keyword updates highlighting through incremental sync, and closing a file drops its state.
- [Visible-range highlighting](./range-highlighting.md): Zed requests tokens for just the viewport.
- [Setup hint](./setup-hint.md): a one-time toast when semantic tokens are off or unsupported, and silence when they work.
- [Server startup](./server-startup.md): advertised capabilities, the legend, and the version line in Zed's LSP log.
