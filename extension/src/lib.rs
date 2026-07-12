use zed_extension_api::{self as zed, Architecture, DownloadedFileType, LanguageServerId, Os, Result};

/// Version of the LSP binary to download from GitHub Releases.
/// Must match the release tag (without the leading "v") when publishing.
/// Update all four version fields together — see CLAUDE.md.
const LSP_VERSION: &str = "0.3.0";

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
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary_path = self.resolve_binary(language_server_id, worktree)?;
        Ok(zed::Command {
            command: binary_path,
            args: vec![],
            env: vec![],
        })
    }
}

impl TodoHighlightExtension {
    /// Resolves the LSP binary using a four-step strategy:
    /// 0. User-configured path from Zed LSP settings (skips all other steps).
    /// 1. In-memory cache (avoids repeated lookups when multiple worktrees are open).
    /// 2. Host PATH lookup (covers local development and user-managed installs).
    /// 3. Download from GitHub Releases via the Zed extension API.
    fn resolve_binary(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<String> {
        // Step 0: honour explicit user override from Zed LSP settings:
        //   { "lsp": { "todo-highlight-lsp": { "binary": { "path": "/custom/path" } } } }
        if let Ok(lsp_settings) = zed::settings::LspSettings::for_worktree("todo-highlight-lsp", worktree) {
            if let Some(binary) = lsp_settings.binary {
                if let Some(path) = binary.path {
                    eprintln!("todo-highlight: using user-configured binary: {path}");
                    return Ok(path);
                }
            }
        }

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
        let path = self.download_binary(language_server_id)?;
        eprintln!("todo-highlight: binary ready at {path}");
        self.cached_binary_path = Some(path.clone());
        Ok(path)
    }

    fn download_binary(&self, language_server_id: &LanguageServerId) -> Result<String> {
        let (os, arch) = zed::current_platform();

        // Linux uses musl targets to produce fully-static binaries that run on any
        // distro (NixOS, Alpine, etc.) without relying on a system glibc.
        let (target, exe_suffix) = match (os, arch) {
            (Os::Mac, Architecture::Aarch64)     => ("aarch64-apple-darwin",        ""),
            (Os::Mac, Architecture::X8664)       => ("x86_64-apple-darwin",         ""),
            (Os::Linux, Architecture::Aarch64)   => ("aarch64-unknown-linux-musl",  ""),
            (Os::Linux, Architecture::X8664)     => ("x86_64-unknown-linux-musl",   ""),
            (Os::Windows, Architecture::X8664)   => ("x86_64-pc-windows-msvc",  ".exe"),
            (Os::Windows, Architecture::Aarch64) => ("aarch64-pc-windows-msvc", ".exe"),
            (os, arch) => {
                return Err(format!(
                    "todo-highlight-lsp has no pre-built binary for {os:?}/{arch:?}. \
                     Build from source: cargo build --release -p todo-highlight-lsp"
                ));
            }
        };

        // Windows ships as .zip; macOS and Linux ship as .tar.gz.
        let (archive_ext, file_type) = if exe_suffix.is_empty() {
            (".tar.gz", DownloadedFileType::GzipTar)
        } else {
            (".zip", DownloadedFileType::Zip)
        };

        let asset_name = format!("todo-highlight-lsp-{target}{archive_ext}");

        // Version-specific directory: lets download_file extract the archive in place,
        // and lets cleanup_old_versions identify stale directories to remove.
        let version_dir = format!("bin/todo-highlight-lsp-{target}-v{LSP_VERSION}");
        let binary_path = format!("{version_dir}/todo-highlight-lsp{exe_suffix}");

        if !std::path::Path::new(&binary_path).exists() {
            self.cleanup_old_versions(target)?;

            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::CheckingForUpdate,
            );

            let release = zed::github_release_by_tag_name(
                "shionit/zed-todo-highlight",
                &format!("v{LSP_VERSION}"),
            );
            let release = match release {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!("Failed to fetch release v{LSP_VERSION}: {e}");
                    zed::set_language_server_installation_status(
                        language_server_id,
                        &zed::LanguageServerInstallationStatus::Failed(msg.clone()),
                    );
                    return Err(msg);
                }
            };

            let asset = match release.assets.iter().find(|a| a.name == asset_name) {
                Some(a) => a,
                None => {
                    let msg = format!("no asset '{asset_name}' in release v{LSP_VERSION}");
                    zed::set_language_server_installation_status(
                        language_server_id,
                        &zed::LanguageServerInstallationStatus::Failed(msg.clone()),
                    );
                    return Err(msg);
                }
            };

            // `download_file` requires the parent directory to exist.
            std::fs::create_dir_all("bin")
                .map_err(|e| format!("failed to create 'bin' directory: {e}"))?;

            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            // For GzipTar/Zip, download_file extracts the archive into version_dir,
            // placing the binary at version_dir/todo-highlight-lsp[.exe].
            if let Err(e) = zed::download_file(&asset.download_url, &version_dir, file_type) {
                let msg = format!("Failed to download {asset_name} v{LSP_VERSION}: {e}");
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::Failed(msg.clone()),
                );
                return Err(msg);
            }

            // `make_file_executable` is a no-op on Windows; safe to call on all platforms.
            zed::make_file_executable(&binary_path)?;
        }

        Ok(binary_path)
    }

    /// Removes version directories for `target` that don't match the current LSP_VERSION.
    /// Silently ignores missing or unreadable directories.
    fn cleanup_old_versions(&self, target: &str) -> Result<()> {
        let current = format!("todo-highlight-lsp-{target}-v{LSP_VERSION}");
        let prefix = format!("todo-highlight-lsp-{target}-v");
        let Ok(entries) = std::fs::read_dir("bin") else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&prefix) && name != current {
                std::fs::remove_dir_all(entry.path()).ok();
            }
        }
        Ok(())
    }
}

zed::register_extension!(TodoHighlightExtension);
