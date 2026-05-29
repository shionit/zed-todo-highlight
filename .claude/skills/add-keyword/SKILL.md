---
name: add-keyword
description: Add a new highlight keyword to todo-highlight end-to-end
---

Add the keyword: $ARGUMENTS

Complete all steps below in order. Do not skip any — each touches a different file and missing one leaves the keyword partially wired.

## Step 1 — LSP server: register the keyword

File: `lsp/src/main.rs`, function `build_keywords()`.

Add a new entry to the `definitions` array:
```rust
("KEYWORD", "keywordKeyword"),
```

Convention: token type name is the keyword in lowercase + `"Keyword"` suffix (e.g. `IDEA` → `ideaKeyword`).

## Step 2 — Default color: semantic_token_rules.json

File: `languages/all/semantic_token_rules.json`.

Add a new object to the JSON array:
```json
{ "token_type": "keywordKeyword", "foreground_color": "#RRGGBB", "font_weight": "bold" }
```

Pick a color that doesn't clash with the existing set. Current palette:
- Orange `#FF8C00` — TODO
- Red `#FF2D55` — FIXME
- Yellow `#FFD60A` — HACK
- Blue `#0A84FF` — NOTE, INFO
- Amber `#FF9F0A` — WARN, WARNING
- Red `#FF453A` — BUG
- Purple `#BF5AF2` — XXX
- Gray `#98989D` — DEPRECATED

## Step 3 — Tests: update the keyword count assertion

File: `lsp/src/main.rs`, test `test_all_keywords`.

Add the new keyword to the test string and increment the expected count:
```rust
let text = "TODO FIXME HACK NOTE INFO WARN WARNING BUG XXX DEPRECATED KEYWORD";
let tokens = scan_tokens(text, &kws);
assert_eq!(tokens.len(), 11); // was 10
```

## Step 4 — README: update the keyword table

File: `README.md`.

Add a row to the keyword/color table. Match the format of existing rows.

## Step 5 — Verify

```bash
./check.sh
```

All 4 steps must be done before running this. If any test fails, fix it before continuing.
