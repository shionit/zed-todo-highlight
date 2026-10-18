# Live editing

As the user types or deletes text, highlighting follows: a newly typed `TODO` lights up and a deleted one disappears. Zed sends only the changed range (incremental sync), and the server rebuilds the text and invalidates its per-document cache.

## Sub-features

- `edit-insert` highlights a keyword inserted mid-line at the correct position.
- `edit-delete` removes the token when the keyword is deleted.
- `edit-close` forgets a document after `didClose`, so later token requests return empty.

## How to get to it (user POV)

- Type `TODO ` into a comment in an open file, then undo it.
- Close the tab.

## Driving it with lsp-drive.py

Preconditions:

- Doctor is OK.

- **Insert, delete, close.** Run `$H run edit`. Expect `[PASS] edit`. In `result.json`, `record.before` is `[]`, `record.after_insert` is a single token at `line 1, col 3` with modifier `todo`, and `record.after_delete` and `record.after_close` are both `[]`.
- **Proof.** The two `textDocument/didChange` frames in `edit/transcript.jsonl` carry only a range and its replacement text, never the full document. Each one is followed by a `semanticTokens/full` response that reflects the edit.

## Gotchas

- The edit ranges are UTF-16 positions. A change to `lsp_pos_to_byte` can pass this ASCII-only scenario while breaking non-ASCII edits. Add a non-ASCII edit scenario when touching that code.
- A stale cache would show up as `after_delete` still containing the token. That is the regression this scenario guards.
