# Setup hint

If the user installed the extension but never enabled `"semantic_tokens": "combined"`, nothing highlights. To explain why, the server shows a one-time info toast after two files have been opened and the user then edits or closes one without Zed ever requesting tokens. On a Zed too old to support semantic tokens, the toast says to update Zed instead. When highlighting works, no toast ever appears (issue #15 was a false toast).

## Sub-features

- `hint-off` sends exactly one `window/showMessage` mentioning `"semantic_tokens": "combined"`, and only after a didChange or didClose that follows 2 or more didOpens.
- `hint-quiet-open` sends no hint while only didOpens have arrived, even if 2 or more files are open.
- `hint-on` never sends a hint once any token request has arrived.
- `hint-unsupported` sends one "update Zed" hint on the first didClose when the client didn't advertise semantic tokens.

## How to get to it (user POV)

- Remove `semantic_tokens` from the global Zed settings, open two files, then close one. A toast appears.
- With the setting enabled, do the same. No toast appears.

## Driving it with lsp-drive.py

Preconditions:

- Doctor is OK.

- **Setting off.** Run `$H run hint-off`. Expect `[PASS] hint-off`. `record.shown_before_close` is `[]` and `record.shown_messages` holds one string containing `"semantic_tokens": "combined"`, even though two didCloses were sent.
- **Setting on.** Run `$H run hint-on`. Expect `[PASS] hint-on` with `record.shown_messages == []`.
- **Old Zed.** Run `$H run hint-unsupported`. Expect `[PASS] hint-unsupported` with one message containing `update Zed`.
- **Proof.** Cite the `window/showMessage` frame (or its absence) in each scenario's `transcript.jsonl`, plus `result.json`.

## Gotchas

- The harness waits 0.3 s (`settle`) for notifications. That is long enough for a local child process. If you see flaky hint counts, raise `settle()` rather than weakening the check.
- The `workspace/semanticTokens/refresh` response must never trigger the hint. The harness answers refresh immediately, which exercises that path in every scenario.
- How the toast looks in Zed is unverified here.
