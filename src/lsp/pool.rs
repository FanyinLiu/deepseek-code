use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use crate::lsp::client::{language_id_for_path, Diagnostic, LspClient};
use crate::storage::config::LspConfig;

const DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(3);
/// The first diagnostics after a cold server start also pay the one-time
/// workspace index (seconds: rust-analyzer runs `cargo metadata`/flycheck
/// before its first diagnostics land), so the first file handled by a
/// freshly-started server gets a longer budget. Paid once per language per
/// session; warm edits use the short timeout above.
const COLD_INDEX_TIMEOUT: Duration = Duration::from_secs(15);

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
            // The config/server key may differ from the document's language id:
            // a `.tsx` file is `typescriptreact` but is normally served by the
            // `typescript` server the user configured. Resolve to the configured
            // key (exact match first, then base language); skip if neither.
            let Some((server_key, command)) = resolve_server(config, language) else {
                continue;
            };
            if self.unavailable.contains(server_key) {
                continue;
            }
            // A server we haven't started yet still owes its first-index cost.
            let cold = !self.servers.contains_key(server_key);
            if !self.ensure_server(server_key, command, project_root).await {
                continue;
            }
            let abs = project_root.join(file);
            let Ok(text) = std::fs::read_to_string(&abs) else {
                continue;
            };
            let abs_str = abs.to_string_lossy().to_string();
            let Some(client) = self.servers.get_mut(server_key) else {
                continue;
            };
            // didOpen still carries the precise language id (e.g. typescriptreact)
            // so the server applies the right rules.
            if client
                .open_or_update(&abs_str, language, &text)
                .await
                .is_err()
            {
                // The server broke mid-stream; drop it and stop using it this run.
                self.servers.remove(server_key);
                self.unavailable.insert(server_key.to_string());
                continue;
            }
            let timeout = if cold {
                COLD_INDEX_TIMEOUT
            } else {
                DIAGNOSTICS_TIMEOUT
            };
            let diags = client.collect_diagnostics(&abs_str, timeout).await;
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

/// The base language whose server also handles a JSX/TSX dialect, so users only
/// need to configure `typescript`/`javascript` to cover `.tsx`/`.jsx` too.
fn base_language(language: &str) -> Option<&'static str> {
    match language {
        "typescriptreact" => Some("typescript"),
        "javascriptreact" => Some("javascript"),
        _ => None,
    }
}

/// Pick the configured server for `language`: exact key first, then the base
/// language. Returns the matched config key and its command.
fn resolve_server<'a>(
    config: &'a LspConfig,
    language: &str,
) -> Option<(&'a str, &'a Vec<String>)> {
    if let Some((key, command)) = config.servers.get_key_value(language) {
        return Some((key.as_str(), command));
    }
    let base = base_language(language)?;
    config
        .servers
        .get_key_value(base)
        .map(|(key, command)| (key.as_str(), command))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(keys: &[&str]) -> LspConfig {
        let mut servers = HashMap::new();
        for key in keys {
            servers.insert((*key).to_string(), vec!["server".to_string()]);
        }
        LspConfig {
            enabled: true,
            servers,
        }
    }

    #[test]
    fn resolve_server_prefers_exact_then_base() {
        // tsx falls back to the typescript server when only `typescript` is set.
        let cfg = config_with(&["typescript"]);
        assert_eq!(
            resolve_server(&cfg, "typescriptreact").map(|(k, _)| k),
            Some("typescript")
        );
        // An exact key wins over the base fallback.
        let cfg = config_with(&["typescript", "typescriptreact"]);
        assert_eq!(
            resolve_server(&cfg, "typescriptreact").map(|(k, _)| k),
            Some("typescriptreact")
        );
        // No matching key at all -> None (server is skipped).
        let cfg = config_with(&["python"]);
        assert!(resolve_server(&cfg, "typescriptreact").is_none());
        assert!(resolve_server(&cfg, "rust").is_none());
    }
}
