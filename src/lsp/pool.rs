use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use crate::lsp::client::{language_id_for_path, Diagnostic, LspClient};
use crate::storage::config::LspConfig;

const DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(3);

/// Reusable pool of language servers for post-edit diagnostics. Servers are
/// started lazily and kept alive, so the costly first index is paid once per
/// language rather than on every edit. Fail-safe throughout: a missing or
/// broken server is recorded and skipped, never failing the caller.
#[derive(Default)]
pub struct LspDiagnosticsPool {
    servers: HashMap<String, LspClient>,
    unavailable: HashSet<String>,
}

impl LspDiagnosticsPool {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Best-effort diagnostics for `files` (paths relative to `project_root`),
    /// grouped by file. Empty when LSP is disabled, no server is configured for
    /// the language, or the server is unavailable.
    pub async fn diagnostics(
        &mut self,
        project_root: &Path,
        files: &[String],
        config: &LspConfig,
    ) -> Vec<(String, Vec<Diagnostic>)> {
        if !config.enabled {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for file in files {
            if !seen.insert(file.clone()) {
                continue;
            }
            let Some(language) = language_id_for_path(file) else {
                continue;
            };
            if self.unavailable.contains(language) {
                continue;
            }
            let Some(command) = config.servers.get(language) else {
                continue;
            };
            if !self.ensure_server(language, command, project_root).await {
                continue;
            }
            let abs = project_root.join(file);
            let Ok(text) = std::fs::read_to_string(&abs) else {
                continue;
            };
            let abs_str = abs.to_string_lossy().to_string();
            let Some(client) = self.servers.get_mut(language) else {
                continue;
            };
            if client.did_open(&abs_str, language, &text).await.is_err() {
                // The server broke mid-stream; drop it and stop using it this run.
                self.servers.remove(language);
                self.unavailable.insert(language.to_string());
                continue;
            }
            let diags = client
                .collect_diagnostics(&abs_str, DIAGNOSTICS_TIMEOUT)
                .await;
            if !diags.is_empty() {
                out.push((file.clone(), diags));
            }
        }
        out
    }

    /// Ensure a server for `language` is running, starting it on first use.
    /// Returns false (and records the language as unavailable) on any failure.
    async fn ensure_server(
        &mut self,
        language: &str,
        command: &[String],
        project_root: &Path,
    ) -> bool {
        if self.servers.contains_key(language) {
            return true;
        }
        let Some((program, args)) = command.split_first() else {
            self.unavailable.insert(language.to_string());
            return false;
        };
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let root_uri = project_root.to_string_lossy();
        match LspClient::start(program, &args, &root_uri).await {
            Ok(client) => {
                self.servers.insert(language.to_string(), client);
                true
            }
            Err(_) => {
                self.unavailable.insert(language.to_string());
                false
            }
        }
    }
}
