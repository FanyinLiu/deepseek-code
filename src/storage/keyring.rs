/// Credential storage using the system keyring, with env var fallback.
///
/// Priority:
/// 1. Provider-specific environment variables
/// 2. System keyring (platform-specific secure storage)
/// 3. Config file (not recommended, but allowed as last resort)
use std::path::{Path, PathBuf};

use crate::provider::ProviderKind;

const KEYRING_SERVICE: &str = "octocode";
const KEYRING_USERNAME: &str = "api-key";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeyStoreLocation {
    Keyring,
    KeyringAndProjectLocalConfig {
        path: PathBuf,
    },
    UserGlobalConfig {
        path: PathBuf,
        keyring_error: String,
    },
    ProjectLocalConfig {
        path: PathBuf,
        keyring_error: String,
    },
}

impl ApiKeyStoreLocation {
    #[must_use]
    pub fn user_message(&self) -> String {
        match self {
            Self::Keyring => "API key stored in system keyring.".to_string(),
            Self::KeyringAndProjectLocalConfig { path } => format!(
                "API key stored in system keyring and local project fallback: {}",
                path.display()
            ),
            Self::UserGlobalConfig {
                path,
                keyring_error,
            } => format!(
                "System keyring was unavailable ({keyring_error}). API key saved to user-global config: {}",
                path.display()
            ),
            Self::ProjectLocalConfig {
                path,
                keyring_error,
            } => format!(
                "System keyring was unavailable ({keyring_error}). API key saved to local project config: {}",
                path.display()
            ),
        }
    }
}

pub fn get_api_key(config_value: Option<&str>) -> Option<String> {
    get_api_key_for_provider(ProviderKind::DeepSeek, config_value)
}

pub fn get_api_key_for_provider(
    provider: ProviderKind,
    config_value: Option<&str>,
) -> Option<String> {
    if let Some(key) = get_env_api_key_for_provider(provider) {
        return Some(key);
    }

    if let Some(key) = get_keyring_api_key() {
        return Some(key);
    }

    get_config_api_key(config_value)
}

pub fn get_api_key_without_keyring(config_value: Option<&str>) -> Option<String> {
    get_api_key_without_keyring_for_provider(ProviderKind::DeepSeek, config_value)
}

pub fn get_api_key_without_keyring_for_provider(
    provider: ProviderKind,
    config_value: Option<&str>,
) -> Option<String> {
    get_env_api_key_for_provider(provider).or_else(|| get_config_api_key(config_value))
}

#[must_use]
pub fn api_key_env_hint(provider: ProviderKind) -> &'static str {
    env_candidates_for_provider(provider)
        .first()
        .copied()
        .unwrap_or("DEEPSEEK_API_KEY")
}

pub fn get_env_api_key() -> Option<String> {
    get_env_api_key_for_provider(ProviderKind::DeepSeek)
}

pub fn get_env_api_key_for_provider(provider: ProviderKind) -> Option<String> {
    for var_name in env_candidates_for_provider(provider) {
        if let Ok(key) = std::env::var(var_name) {
            if !key.trim().is_empty() {
                tracing::debug!("using API key from {} env var", var_name);
                return Some(key.trim().to_string());
            }
        }
    }
    None
}

fn env_candidates_for_provider(provider: ProviderKind) -> &'static [&'static str] {
    match provider {
        ProviderKind::DeepSeek => &["DEEPSEEK_API_KEY"],
        ProviderKind::Qwen => &["DASHSCOPE_API_KEY", "BAILIAN_API_KEY", "QWEN_API_KEY"],
        ProviderKind::Kimi => &["MOONSHOT_API_KEY", "KIMI_API_KEY"],
        ProviderKind::Zhipu => &["ZAI_API_KEY", "ZHIPUAI_API_KEY", "ZHIPU_API_KEY"],
        ProviderKind::Minimax => &["MINIMAX_API_KEY"],
        ProviderKind::Tencent => &["TENCENT_TOKENHUB_API_KEY", "HUNYUAN_API_KEY"],
        ProviderKind::Qianfan => &["QIANFAN_API_KEY", "BAIDU_QIANFAN_API_KEY"],
        ProviderKind::Stepfun => &["STEPFUN_KEY", "STEPFUN_API_KEY"],
        ProviderKind::Doubao => &["ARK_API_KEY", "VOLCENGINE_API_KEY", "DOUBAO_API_KEY"],
        ProviderKind::OpenRouter => &["OPENROUTER_API_KEY"],
        ProviderKind::OpenAiCompatible => &["OPENAI_API_KEY"],
    }
}

pub fn get_keyring_api_key() -> Option<String> {
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USERNAME) {
        Ok(entry) => match entry.get_password() {
            Ok(key) if !key.is_empty() => {
                tracing::debug!("using API key from system keyring");
                return Some(key);
            }
            _ => {}
        },
        Err(e) => {
            tracing::debug!("keyring not available: {}", e);
        }
    }
    None
}

fn get_config_api_key(config_value: Option<&str>) -> Option<String> {
    if let Some(key) = config_value {
        if !key.trim().is_empty() {
            tracing::warn!("using API key from config file — consider using keyring instead");
            return Some(key.trim().to_string());
        }
    }

    None
}

pub fn get_effective_api_key(project_root: Option<&Path>) -> Option<String> {
    let config = project_root.and_then(|root| crate::storage::Config::load(Some(root)).ok());
    let provider = config
        .as_ref()
        .map(|config| config.provider.default)
        .unwrap_or_default();
    let config_value = config.as_ref().and_then(config_api_key);
    get_api_key_for_provider(provider, config_value)
}

pub fn config_api_key(config: &crate::storage::Config) -> Option<&str> {
    config
        .api_key
        .as_deref()
        .or_else(|| {
            config
                .profiles
                .get("default")
                .and_then(|profile| profile.api_key.as_deref())
        })
        .or_else(|| {
            config
                .profiles
                .values()
                .find_map(|profile| profile.api_key.as_deref())
        })
}

pub fn store_api_key(key: &str) -> Result<(), anyhow::Error> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USERNAME)?;
    entry.set_password(key.trim())?;
    tracing::info!("API key stored in system keyring");
    Ok(())
}

pub fn store_api_key_with_project_fallback(
    key: &str,
    project_root: Option<&Path>,
) -> Result<ApiKeyStoreLocation, anyhow::Error> {
    let trimmed = key.trim();
    let keyring_error = match store_api_key(trimmed) {
        Ok(()) => {
            // Verify the key can be read back (Windows keyring sometimes lies about success)
            match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USERNAME) {
                Ok(entry) => match entry.get_password() {
                    Ok(read_back) if read_back == trimmed => {
                        tracing::info!("API key stored and verified in system keyring");
                        if let Some(location) =
                            maybe_store_windows_project_fallback(project_root, trimmed)?
                        {
                            return Ok(location);
                        }
                        return Ok(ApiKeyStoreLocation::Keyring);
                    }
                    Ok(_) => {
                        let message = "keyring stored a different key than expected".to_string();
                        tracing::warn!("{message}");
                        message
                    }
                    Err(e) => {
                        let message = format!("keyring write succeeded but read-back failed: {e}");
                        tracing::warn!("{message}");
                        message
                    }
                },
                Err(e) => {
                    let message = format!("keyring entry not accessible after write: {e}");
                    tracing::warn!("{message}");
                    message
                }
            }
        }
        Err(error) => {
            tracing::warn!("keyring store failed: {error}");
            error.to_string()
        }
    };

    tracing::info!("falling back to user-global config after keyring verification failure");

    // Prefer user-global ~/.octocode/config.toml so the key works from any cwd.
    // Only fall back to project-local when there is no home dir.
    if let Some(path) = store_api_key_in_user_global_config(trimmed)? {
        return Ok(ApiKeyStoreLocation::UserGlobalConfig {
            path,
            keyring_error,
        });
    }

    let Some(root) = project_root else {
        return Err(anyhow::anyhow!(
            "failed to store API key in system keyring and no home or project root available for fallback"
        ));
    };
    let path = store_api_key_in_project_local_config(root, trimmed)?;
    Ok(ApiKeyStoreLocation::ProjectLocalConfig {
        path,
        keyring_error,
    })
}

/// Store the API key in `~/.octocode/config.toml` (user-global config).
/// Returns `Ok(None)` if no home directory is available.
pub fn store_api_key_in_user_global_config(key: &str) -> Result<Option<PathBuf>, anyhow::Error> {
    let Some(config_dir) = crate::storage::user_config_dir() else {
        return Ok(None);
    };
    std::fs::create_dir_all(&config_dir)?;
    let path = config_dir.join("config.toml");

    let mut table = if path.exists() {
        let content = crate::storage::read_text_file_capped(&path)?;
        let value: toml::Value = toml::from_str(&content)?;
        value
            .as_table()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{} is not a TOML table", path.display()))?
    } else {
        toml::map::Map::new()
    };

    table.insert(
        "api_key".to_string(),
        toml::Value::String(key.trim().to_string()),
    );
    let rendered = toml::to_string_pretty(&toml::Value::Table(table))?;
    write_api_key_config(&path, &rendered)?;
    tracing::warn!("API key stored in user-global config: {}", path.display());
    Ok(Some(path))
}

fn maybe_store_windows_project_fallback(
    project_root: Option<&Path>,
    key: &str,
) -> Result<Option<ApiKeyStoreLocation>, anyhow::Error> {
    if !cfg!(target_os = "windows") {
        return Ok(None);
    }

    let Some(root) = project_root else {
        return Ok(None);
    };

    let path = store_api_key_in_project_local_config(root, key)?;
    Ok(Some(ApiKeyStoreLocation::KeyringAndProjectLocalConfig {
        path,
    }))
}

pub fn store_api_key_in_project_local_config(
    project_root: &Path,
    key: &str,
) -> Result<PathBuf, anyhow::Error> {
    let project_root = crate::storage::config::normalize_project_root(project_root);
    let config_dir = project_root.join(".octocode");
    std::fs::create_dir_all(&config_dir)?;
    let path = config_dir.join("local.toml");

    let mut table = if path.exists() {
        let content = crate::storage::read_text_file_capped(&path)?;
        let value: toml::Value = toml::from_str(&content)?;
        value
            .as_table()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{} is not a TOML table", path.display()))?
    } else {
        toml::map::Map::new()
    };

    table.insert(
        "api_key".to_string(),
        toml::Value::String(key.trim().to_string()),
    );
    let rendered = toml::to_string_pretty(&toml::Value::Table(table))?;
    write_api_key_config(&path, &rendered)?;
    tracing::warn!("API key stored in local project config: {}", path.display());
    Ok(path)
}

fn write_api_key_config(path: &Path, rendered: &str) -> Result<(), anyhow::Error> {
    crate::storage::atomic::write_private_text_atomic(path, rendered)
}

pub fn delete_api_key() -> Result<(), anyhow::Error> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USERNAME)?;
    match entry.delete_credential() {
        Ok(()) => {
            tracing::info!("API key removed from system keyring");
            Ok(())
        }
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_local_config_api_key_is_created() {
        let root = tempfile::tempdir().expect("tempdir");

        let path =
            store_api_key_in_project_local_config(root.path(), "sk-local").expect("store api key");

        assert_eq!(path, root.path().join(".octocode").join("local.toml"));
        let loaded = crate::storage::Config::load(Some(root.path())).expect("load config");
        assert_eq!(loaded.api_key.as_deref(), Some("sk-local"));
    }

    #[test]
    fn project_local_config_normalizes_config_dir_root() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".octocode");
        std::fs::create_dir_all(&config_dir).expect("create config dir");

        let path =
            store_api_key_in_project_local_config(&config_dir, "sk-local").expect("store api key");

        assert_eq!(path, config_dir.join("local.toml"));
        assert!(!config_dir.join(".octocode").exists());
        let loaded = crate::storage::Config::load(Some(&config_dir)).expect("load config");
        assert_eq!(loaded.api_key.as_deref(), Some("sk-local"));
    }

    #[test]
    fn project_local_config_preserves_existing_sections() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".octocode");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("local.toml"),
            "[ui]\nshow_cache_hud = false\n\n[router]\nsimple_threshold = 55\n",
        )
        .expect("write local config");

        store_api_key_in_project_local_config(root.path(), "sk-local").expect("store api key");

        let loaded = crate::storage::Config::load(Some(root.path())).expect("load config");
        assert_eq!(loaded.api_key.as_deref(), Some("sk-local"));
        assert!(!loaded.ui.show_cache_hud);
        assert_eq!(loaded.router.simple_threshold, 55);
    }

    #[cfg(unix)]
    #[test]
    fn project_local_api_key_file_is_owner_only_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".octocode");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        let local = config_dir.join("local.toml");
        std::fs::write(&local, "[ui]\ntheme = \"light\"\n").expect("write local config");
        std::fs::set_permissions(&local, std::fs::Permissions::from_mode(0o644))
            .expect("loosen local config permissions");

        store_api_key_in_project_local_config(root.path(), "sk-local").expect("store api key");

        let mode = std::fs::metadata(&local)
            .expect("local metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn project_local_api_key_does_not_reset_project_config() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".octocode");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            "[router]\nsimple_threshold = 55\n\n[ui]\nshow_cache_hud = false\n",
        )
        .expect("write project config");

        store_api_key_in_project_local_config(root.path(), "sk-local").expect("store api key");

        let loaded = crate::storage::Config::load(Some(root.path())).expect("load config");
        assert_eq!(loaded.api_key.as_deref(), Some("sk-local"));
        assert_eq!(loaded.router.simple_threshold, 55);
        assert!(!loaded.ui.show_cache_hud);
    }

    #[test]
    fn api_key_env_hints_cover_chinese_provider_profiles() {
        assert_eq!(api_key_env_hint(ProviderKind::Minimax), "MINIMAX_API_KEY");
        assert_eq!(
            api_key_env_hint(ProviderKind::Tencent),
            "TENCENT_TOKENHUB_API_KEY"
        );
        assert_eq!(api_key_env_hint(ProviderKind::Qianfan), "QIANFAN_API_KEY");
        assert_eq!(api_key_env_hint(ProviderKind::Stepfun), "STEPFUN_KEY");
        assert_eq!(api_key_env_hint(ProviderKind::Doubao), "ARK_API_KEY");
    }

    #[test]
    fn windows_project_fallback_is_written_when_keyring_verifies() {
        let root = tempfile::tempdir().expect("tempdir");

        let location =
            maybe_store_windows_project_fallback(Some(root.path()), "sk-local").expect("fallback");

        if cfg!(target_os = "windows") {
            assert!(matches!(
                location,
                Some(ApiKeyStoreLocation::KeyringAndProjectLocalConfig { .. })
            ));
            let loaded = crate::storage::Config::load(Some(root.path())).expect("load config");
            assert_eq!(loaded.api_key.as_deref(), Some("sk-local"));
        } else {
            assert_eq!(location, None);
        }
    }
}
