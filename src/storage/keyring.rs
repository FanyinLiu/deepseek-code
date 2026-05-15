/// Credential storage using the system keyring, with env var fallback.
///
/// Priority:
/// 1. `DEEPSEEK_API_KEY` environment variable
/// 2. System keyring (platform-specific secure storage)
/// 3. Config file (not recommended, but allowed as last resort)
use std::path::{Path, PathBuf};

const KEYRING_SERVICE: &str = "deepseek-code";
const KEYRING_USERNAME: &str = "api-key";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeyStoreLocation {
    Keyring,
    KeyringAndProjectLocalConfig {
        path: PathBuf,
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
    if let Some(key) = get_env_api_key() {
        return Some(key);
    }

    if let Some(key) = get_keyring_api_key() {
        return Some(key);
    }

    get_config_api_key(config_value)
}

pub fn get_api_key_without_keyring(config_value: Option<&str>) -> Option<String> {
    get_env_api_key().or_else(|| get_config_api_key(config_value))
}

pub fn get_env_api_key() -> Option<String> {
    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
        if !key.trim().is_empty() {
            tracing::debug!("using API key from DEEPSEEK_API_KEY env var");
            return Some(key.trim().to_string());
        }
    }
    None
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
    let config_value = config.as_ref().and_then(config_api_key);
    get_api_key(config_value)
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

    tracing::info!("falling back to project-local config after keyring verification failure");

    let Some(root) = project_root else {
        return Err(anyhow::anyhow!(
            "failed to store API key in system keyring and no project root was available for local fallback"
        ));
    };
    let path = store_api_key_in_project_local_config(root, trimmed)?;
    Ok(ApiKeyStoreLocation::ProjectLocalConfig {
        path,
        keyring_error,
    })
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
    let config_dir = project_root.join(".deepseek-code");
    std::fs::create_dir_all(&config_dir)?;
    let path = config_dir.join("local.toml");

    let mut table = if path.exists() {
        let content = std::fs::read_to_string(&path)?;
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
    std::fs::write(&path, rendered)?;
    tracing::warn!("API key stored in local project config: {}", path.display());
    Ok(path)
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

        assert_eq!(path, root.path().join(".deepseek-code").join("local.toml"));
        let loaded = crate::storage::Config::load(Some(root.path())).expect("load config");
        assert_eq!(loaded.api_key.as_deref(), Some("sk-local"));
    }

    #[test]
    fn project_local_config_normalizes_config_dir_root() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".deepseek-code");
        std::fs::create_dir_all(&config_dir).expect("create config dir");

        let path =
            store_api_key_in_project_local_config(&config_dir, "sk-local").expect("store api key");

        assert_eq!(path, config_dir.join("local.toml"));
        assert!(!config_dir.join(".deepseek-code").exists());
        let loaded = crate::storage::Config::load(Some(&config_dir)).expect("load config");
        assert_eq!(loaded.api_key.as_deref(), Some("sk-local"));
    }

    #[test]
    fn project_local_config_preserves_existing_sections() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".deepseek-code");
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

    #[test]
    fn project_local_api_key_does_not_reset_project_config() {
        let root = tempfile::tempdir().expect("tempdir");
        let config_dir = root.path().join(".deepseek-code");
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
