# Keyword highlighting

When a user opens a file in a supported language, every whole-word occurrence of `TODO`, `FIXME`, `HACK`, `NOTE`, `INFO`, `WARN`, `WARNING`, `BUG`, `XXX`, and `DEPRECATED` is highlighted in the theme's `keyword` style. Each one carries a lowercase modifier (`todo`, `fixme`, and so on) so users can give keywords their own colors.

## Sub-features

- `hl-all` highlights all 10 keywords in document order.
- `hl-type` makes every token the standard `keyword` type, so highlighting works with no color rules.
- `hl-modifier` sets each token's only modifier to the lowercased keyword.
- `hl-boundary` skips matches inside longer words (`TODOS`, `MYTODO`).
- `hl-utf16` reports columns in UTF-16 code units, so lines with CJK or emoji highlight the right span.

## How to get to it (user POV)

- Open any file whose language is listed in `extension/extension.toml` while `"semantic_tokens": "combined"` is set.
- Zed sends `textDocument/didOpen`, then `textDocument/semanticTokens/full`.

## Driving it with lsp-drive.py

Preconditions:

- Doctor is OK.

- **All keywords.** Run `$H run highlight`. Expect `[PASS] highlight`. In `result.json`, `record.tokens[*].text` lists the 10 keywords in order, each with `type: "keyword"` and `modifiers: ["<lowercase>"]`, and there is no token on line 3 (`TODOS and MYTODO`).
- **Non-ASCII columns.** Run `$H run unicode`. Expect `[PASS] unicode`. Tokens are at `(col 7, len 4)` and `(col 15, len 5)` on `// 日本語 TODO 🎉 FIXME`.
- **Proof.** Cite `highlight/result.json`, which holds the decoded tokens with their matched text, and the `semanticTokens/full` request/response pair in `highlight/transcript.jsonl`.

## Gotchas

- Adding a keyword (`/add-keyword`) changes the expected list in `sc_highlight`. Update the scenario in the same change, or it will FAIL for the right reason.
- Matching is case-sensitive: `todo` in lowercase is not highlighted, by design.
- A passing run proves which tokens Zed receives, not which color it paints. Per-keyword colors depend on the user's global `semantic_token_rules` and stay unverified here.
