use zed_extension_api::{self as zed, Architecture, DownloadedFileType, LanguageServerId, Os, Result};

/// Version of the LSP binary to download from GitHub Releases.
/// Must match the release tag (without the leading "v") when publishing.
/// Update all four version fields together — see CLAUDE.md.
const LSP_VERSION: &str = "0.1.0";

struct TodoHighlightExtension {
    cached_binary_path: Option<String>,
}

impl zed::Extension for TodoHighlightExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary_path = self.resolve_binary(worktree)?;
        Ok(zed::Command {
            command: binary_path,
            args: vec![],
            env: vec![],
        })
    }
}

impl TodoHighlightExtension {
    /// Resolves the LSP binary using a three-step strategy:
    /// 1. In-memory cache (avoids repeated lookups when multiple worktrees are open).
    /// 2. Host PATH lookup (covers local development and user-managed installs).
    /// 3. Download from GitHub Releases via the Zed extension API.
    fn resolve_binary(&mut self, worktree: &zed::Worktree) -> Result<String> {
        // Step 1: return cached path if binary still exists on disk.
        if let Some(path) = &self.cached_binary_path {
            if std::path::Path::new(path).exists() {
                return Ok(path.clone());
            }
            eprintln!("todo-highlight: cached binary gone, re-resolving");
            self.cached_binary_path = None;
        }

        // Step 2: use a locally installed binary (development / manual install).
        // `worktree.which()` is proxied through the Zed host runtime so it works
        // correctly inside the WASM sandbox.
        if let Some(path) = worktree.which("todo-highlight-lsp") {
            eprintln!("todo-highlight: using binary from PATH: {path}");
            self.cached_binary_path = Some(path.clone());
            return Ok(path);
        }

        // Step 3: download a pre-built binary from GitHub Releases.
        eprintln!("todo-highlight: binary not in PATH, attempting download v{LSP_VERSION}");
        let path = self.download_binary()?;
        eprintln!("todo-highlight: binary ready at {path}");
        self.cached_binary_path = Some(path.clone());
        Ok(path)
    }

    fn download_binary(&self) -> Result<String> {
        let (os, arch) = zed::current_platform();

        // `exe_suffix` is ".exe" on Windows and "" everywhere else.
        let (target, exe_suffix) = match (os, arch) {
            (Os::Mac, Architecture::Aarch64)     => ("aarch64-apple-darwin",           ""),
            (Os::Mac, Architecture::X8664)       => ("x86_64-apple-darwin",            ""),
            (Os::Linux, Architecture::Aarch64)   => ("aarch64-unknown-linux-gnu",      ""),
            (Os::Linux, Architecture::X8664)     => ("x86_64-unknown-linux-gnu",       ""),
            (Os::Windows, Architecture::X8664)   => ("x86_64-pc-windows-msvc",   ".exe"),
            (Os::Windows, Architecture::Aarch64) => ("aarch64-pc-windows-msvc",  ".exe"),
            (os, arch) => {
                return Err(format!(
                    "todo-highlight-lsp has no pre-built binary for {os:?}/{arch:?}. \
                     Build from source: cargo build --release -p todo-highlight-lsp"
                ));
            }
        };

        let binary_name = format!("todo-highlight-lsp-{target}{exe_suffix}");

        // Version in the path prevents a stale cached binary from being reused
        // after an extension upgrade.
        let binary_path = format!("bin/todo-highlight-lsp-{target}-v{LSP_VERSION}{exe_suffix}");

        if !std::path::Path::new(&binary_path).exists() {
            let release = zed::github_release_by_tag_name(
                "shionit/zed-todo-highlight",
                &format!("v{LSP_VERSION}"),
            )?;
            let asset = release
                .assets
                .iter()
                .find(|a| a.name == binary_name)
                .ok_or_else(|| format!("no asset '{binary_name}' in release v{LSP_VERSION}"))?;
            zed::download_file(&asset.download_url, &binary_path, DownloadedFileType::Uncompressed)
                .map_err(|e| format!("Failed to download {binary_name} v{LSP_VERSION}: {e}"))?;
            // `make_file_executable` is a no-op on Windows; safe to call on all platforms.
            zed::make_file_executable(&binary_path)?;
        }

        Ok(binary_path)
    }
}

zed::register_extension!(TodoHighlightExtension);
