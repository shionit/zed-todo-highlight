use zed_extension_api::{self as zed, Architecture, DownloadedFileType, LanguageServerId, Os, Result};

/// Version of the LSP binary to download from GitHub Releases.
/// Must match the release tag (without the leading "v") when publishing.
/// Update all four version fields together — see CLAUDE.md.
const LSP_VERSION: &str = "0.1.0";

struct TodoHighlighterExtension {
    cached_binary_path: Option<String>,
}

impl zed::Extension for TodoHighlighterExtension {
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

impl TodoHighlighterExtension {
    /// Resolves the LSP binary path using a three-step strategy:
    /// 1. In-memory cache (avoids repeated lookups when multiple worktrees are open).
    /// 2. Host PATH lookup (covers local development and user-managed installs).
    /// 3. Download from GitHub Releases over HTTPS.
    fn resolve_binary(&mut self, worktree: &zed::Worktree) -> Result<String> {
        // Step 1: return cached path if binary still exists on disk.
        if let Some(path) = &self.cached_binary_path {
            if std::path::Path::new(path).exists() {
                return Ok(path.clone());
            }
            eprintln!("todo-highlighter: cached binary gone, re-resolving");
            self.cached_binary_path = None;
        }

        // Step 2: use a locally installed binary (development / manual install).
        // `worktree.which()` is proxied through the Zed host runtime so it works
        // correctly inside the WASM sandbox.
        if let Some(path) = worktree.which("todo-highlighter-lsp") {
            eprintln!("todo-highlighter: using binary from PATH: {path}");
            self.cached_binary_path = Some(path.clone());
            return Ok(path);
        }

        // Step 3: download a pre-built binary from GitHub Releases.
        // Integrity is provided by HTTPS (TLS) — GitHub releases use a CDN
        // with TLS certificates. The cpufeatures crate required by sha2 does
        // not compile in Zed's WASM sandbox, so in-process hash verification
        // is not used here.
        eprintln!("todo-highlighter: binary not in PATH, attempting download v{LSP_VERSION}");
        let path = self.download_binary()?;
        eprintln!("todo-highlighter: binary ready at {path}");
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
                    "todo-highlighter-lsp has no pre-built binary for {os:?}/{arch:?}. \
                     Build from source: cargo build --release -p todo-highlighter-lsp"
                ));
            }
        };

        let binary_name = format!("todo-highlighter-lsp-{target}{exe_suffix}");

        // Version in the path prevents a stale cached binary from being reused
        // after an extension upgrade.
        let binary_path = format!("bin/todo-highlighter-lsp-{target}-v{LSP_VERSION}{exe_suffix}");

        if !std::path::Path::new(&binary_path).exists() {
            let url = format!(
                "https://github.com/shionit/todo-highlighter/releases/download/v{LSP_VERSION}/{binary_name}"
            );
            zed::download_file(&url, &binary_path, DownloadedFileType::Uncompressed)
                .map_err(|e| format!("Failed to download {binary_name} v{LSP_VERSION}: {e}"))?;
            // `make_file_executable` is a no-op on Windows; safe to call on all platforms.
            zed::make_file_executable(&binary_path)?;
        }

        Ok(binary_path)
    }
}

zed::register_extension!(TodoHighlighterExtension);
