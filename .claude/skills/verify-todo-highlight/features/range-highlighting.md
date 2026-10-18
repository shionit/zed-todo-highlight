# Visible-range highlighting

For large files Zed can ask for tokens in just the visible range instead of the whole document. The server returns only the keywords inside that range, with positions still measured from the start of the document.

## Sub-features

- `range-filter` returns only keywords whose start is in `[start, end)`.
- `range-origin` delta-encodes the first token from line 0, not from the range start.

## How to get to it (user POV)

- Open a long file and scroll. Zed sends `textDocument/semanticTokens/range` for the viewport.

## Driving it with lsp-drive.py

Preconditions:

- Doctor is OK.

- **Range on line 1.** Run `$H run range`. Expect `[PASS] range`. `record.tokens` is `FIXME` and `HACK`, both on `line: 1`. The `TODO` on line 0 and the keywords on line 2 are excluded.
- **Proof.** The `semanticTokens/range` request in `range/transcript.jsonl` carries the range `{1:0 → 2:0}`, and the response holds exactly two tokens.

## Gotchas

- The range end is exclusive, so a keyword starting exactly at `end` is dropped. That is intentional and covered by unit tests.
- Whether Zed actually uses range requests (versus full) for a given file is Zed's choice and is not observable here.
