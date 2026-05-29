# TODO Highlight for Zed

Highlights `TODO`, `FIXME`, `HACK`, `NOTE`, `WARN`, `BUG` and other keywords across all file types in Zed.

![TODO Highlight in action](docs/screenshot.png)

Inspired by the VS Code [TODO Highlight](https://marketplace.visualstudio.com/items?itemName=wayou.vscode-todo-highlight) extension.

## Keywords & Default Colors

| Keyword      | Color      |
|--------------|------------|
| `TODO`       | 🟠 Orange  |
| `FIXME`      | 🔴 Red     |
| `HACK`       | 🟡 Yellow  |
| `NOTE`       | 🔵 Blue    |
| `INFO`       | 🔵 Blue    |
| `WARN`       | 🟡 Amber   |
| `WARNING`    | 🟡 Amber   |
| `BUG`        | 🔴 Red     |
| `XXX`        | 🟣 Purple  |
| `DEPRECATED` | ⚫ Gray    |

## Installation

### 1. Install the extension

Install from the Zed extension registry, or as a dev extension:

1. Build the LSP server binary and add it to your PATH:

   **macOS / Linux**
   ```sh
   cargo build --release -p todo-highlight-lsp
   cp target/release/todo-highlight-lsp ~/.local/bin/
   ```

   **Windows**
   ```powershell
   cargo build --release -p todo-highlight-lsp
   copy target\release\todo-highlight-lsp.exe "$env:USERPROFILE\.local\bin\"
   ```
   (Create `%USERPROFILE%\.local\bin` and add it to your `PATH` if it doesn't exist.)

2. Zed → Extensions → `Install Dev Extension` → select the **`extension/`** subdirectory

### 2. Add required settings

Open your Zed settings (`⌘,`) and add the following block. Both sections are required — `semantic_tokens` tells Zed to use LSP-based highlighting, and `semantic_token_rules` defines the colors for each keyword.

```settings.json
{
  "semantic_tokens": "combined",
  "global_lsp_settings": {
    "semantic_token_rules": [
      { "token_type": "todoKeyword",       "foreground_color": "#FF8C00", "background_color": "#2B1700", "font_weight": "bold" },
      { "token_type": "fixmeKeyword",      "foreground_color": "#FF2D55", "background_color": "#2B0011", "font_weight": "bold" },
      { "token_type": "hackKeyword",       "foreground_color": "#FFD60A", "background_color": "#272100", "font_weight": "bold" },
      { "token_type": "noteKeyword",       "foreground_color": "#0A84FF", "background_color": "#001B2B", "font_weight": "bold" },
      { "token_type": "infoKeyword",       "foreground_color": "#0A84FF", "background_color": "#001B2B", "font_weight": "bold" },
      { "token_type": "warnKeyword",       "foreground_color": "#FF9F0A", "background_color": "#271B00", "font_weight": "bold" },
      { "token_type": "warningKeyword",    "foreground_color": "#FF9F0A", "background_color": "#271B00", "font_weight": "bold" },
      { "token_type": "bugKeyword",        "foreground_color": "#FF453A", "background_color": "#2B0A00", "font_weight": "bold" },
      { "token_type": "xxxKeyword",        "foreground_color": "#BF5AF2", "background_color": "#1B0029", "font_weight": "bold" },
      { "token_type": "deprecatedKeyword", "foreground_color": "#98989D", "background_color": "#1A1A1A", "font_weight": "bold" }
    ]
  }
}
```

> **Why are these settings needed?**
> Zed treats semantic token highlighting as opt-in (`semantic_tokens`) and requires explicit color definitions for custom token types (`semantic_token_rules`). These are Zed-wide settings that extensions cannot set on your behalf. Once added, the settings apply to all projects and never need to be touched again.

## Customizing Colors

### Foreground color

Change any color by editing the `foreground_color` value in your `settings.json`. Standard hex colors are supported.

### Background color

Each rule accepts an optional `background_color` field. The palette in the installation example above uses colors derived from each keyword's foreground hue at low lightness, tuned for dark themes:

| Keyword | Background |
|---------|-----------|
| `TODO` | `#2B1700` — dark orange |
| `FIXME` | `#2B0011` — dark crimson |
| `HACK` | `#272100` — dark yellow |
| `NOTE` / `INFO` | `#001B2B` — dark blue |
| `WARN` / `WARNING` | `#271B00` — dark amber |
| `BUG` | `#2B0A00` — dark red |
| `XXX` | `#1B0029` — dark purple |
| `DEPRECATED` | `#1A1A1A` — near-black |

**Light themes:** swap the dark backgrounds for light tints — e.g. `#FFF0D9` for TODO, `#FFE5E9` for FIXME.

Omit `background_color` from any rule to leave the editor's default background unchanged for that keyword.

## Troubleshooting

**Keywords are not highlighted after installation**

1. Confirm both `semantic_tokens` and `semantic_token_rules` are in your `settings.json` (see Installation step 2 above).
2. Confirm the LSP server is running: Zed → `View` → `Toggle Log` → search for `todo-highlight-lsp`.
3. If the log shows "binary not found", the LSP binary is not on your PATH — see step 1 of the installation.

> If you installed the extension without configuring settings yet, the extension will show a notification after you open two files guiding you to the required settings.

**Only some keywords are highlighted**

The extension uses word-boundary matching (`\bKEYWORD\b`), so `TODOS` will not match `TODO`. Check for extra characters attached to the keyword.

**Colors look wrong or missing**

Zed merges `semantic_token_rules` from your active theme and from `global_lsp_settings`. If a theme already defines a token type name that conflicts, the theme value wins. Try switching to a different theme, or remove conflicting entries from the theme's definition.

## Architecture

```
Zed Editor
  └── Extension (Rust → WASM)   — registers LSP server, downloads binary on install
        └── LSP Server (binary) — regex-scans buffers, emits semantic tokens
```

The LSP server is a lightweight Rust binary with no async runtime. It uses INCREMENTAL text sync to minimise data transfer and caches scan results per document version, so repeated token requests for unchanged files return immediately.
