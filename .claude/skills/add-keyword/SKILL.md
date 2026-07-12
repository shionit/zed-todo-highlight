---
name: add-keyword
description: Add a new highlight keyword to todo-highlight end-to-end
---

Add the keyword: $ARGUMENTS

Complete all steps below in order. Do not skip any — each touches a different file and missing one leaves the keyword partially wired.

## Step 1 — LSP server: register the keyword

File: `lsp/src/keywords.rs`, function `build_keywords()`.

Add a new entry to the `definitions` array:
```rust
("KEYWORD", "keyword"),
```

Convention: the second element is the semantic token **modifier** — the keyword in lowercase (e.g. `IDEA` → `"idea"`). All keywords share the standard `keyword` token type; the modifier bit and legend entry are derived automatically from the array index.

## Step 2 — Tests: update the keyword assertions

File: `lsp/src/main.rs`, test `test_all_keywords`.

Add the new keyword to the test string and increment the expected count:
```rust
let text = "TODO FIXME HACK NOTE INFO WARN WARNING BUG XXX DEPRECATED KEYWORD";
let tokens = scan_tokens(text, &kws);
assert_eq!(tokens.len(), 11); // was 10
```

The per-token modifier-bit loop needs no change — bits are derived from definition order, which matches the text above.

## Step 3 — README: update keyword docs

File: `README.md`.

1. Add the keyword to the list in the **Keywords** section.
2. Add a rule line to the palette example in **Customizing Colors**:
   ```json
   { "token_type": "keyword", "token_modifiers": ["keyword-lowercase"], "foreground_color": "#RRGGBB", "font_weight": "bold" }
   ```
3. If you give it a background color, add a row to the background table.

Pick a color that doesn't clash with the existing set. Current palette:
- Orange `#FF8C00` — TODO
- Red `#FF2D55` — FIXME
- Yellow `#FFD60A` — HACK
- Blue `#0A84FF` — NOTE, INFO
- Amber `#FF9F0A` — WARN, WARNING
- Red `#FF453A` — BUG
- Purple `#BF5AF2` — XXX
- Gray `#98989D` — DEPRECATED

## Step 4 — Verify

```bash
./check.sh
```

All previous steps must be done before running this. If any test fails, fix it before continuing.
