use std::io::IsTerminal;
use std::path::Path;

use crate::provider::ProviderKind;
use crate::storage;

/// Validate an API key entered by the user.
pub fn validate_api_key(key: &str) -> Result<&str, anyhow::Error> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        anyhow::bail!("No API key provided");
    }
    if trimmed.chars().count() < 8 {
        anyhow::bail!("API key looks too short. Got: {}...", preview_key(trimmed));
    }
    Ok(trimmed)
}

fn preview_key(key: &str) -> String {
    key.chars().take(8).collect()
}

/// Resolve an API key from the environment/keyring/config, or prompt once.
pub fn resolve_or_prompt_api_key(project_root: Option<&Path>) -> Result<String, anyhow::Error> {
    if let Some(key) = storage::get_effective_api_key(project_root) {
        return Ok(key);
    }

    prompt_and_store_api_key(project_root)
}

/// Resolve an API key without prompting or printing interactive setup text.
pub fn resolve_api_key_non_interactive(
    project_root: Option<&Path>,
) -> Result<String, anyhow::Error> {
    storage::get_effective_api_key(project_root).ok_or_else(|| {
        anyhow::anyhow!(
            "No API key configured. Run `octo login --api-key sk-...` or set DEEPSEEK_API_KEY."
        )
    })
}

/// Prompt interactively for the API key and store it in keyring or project-local config.
pub fn prompt_and_store_api_key(project_root: Option<&Path>) -> Result<String, anyhow::Error> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "No API key configured. Run `octo login --api-key <key>` or set a provider API key env var."
        );
    }

    let provider = configured_provider(project_root);
    println!("No {} API key configured.", provider.as_str());
    println!(
        "Enter your provider API key, or set {}.",
        storage::api_key_env_hint(provider)
    );
    if provider == ProviderKind::DeepSeek {
        println!("Get one at https://platform.deepseek.com/api_keys");
    }

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let key = validate_api_key(&input)?.to_string();

    let location = storage::store_api_key_with_project_fallback(&key, project_root)?;
    println!("{}", location.user_message());
    Ok(key)
}

fn configured_provider(project_root: Option<&Path>) -> ProviderKind {
    project_root
        .and_then(|root| storage::Config::load(Some(root)).ok())
        .map(|config| config.provider.default)
        .unwrap_or_default()
}

/// Run the login command: store API key in system keyring.
pub async fn login(
    api_key: Option<String>,
    project_root: Option<&Path>,
) -> Result<(), anyhow::Error> {
    if let Some(key) = api_key {
        let trimmed = validate_api_key(&key)?;
        let location = storage::store_api_key_with_project_fallback(trimmed, project_root)?;
        println!("✅ {}", location.user_message());
        println!("   Run `octo doctor` to verify.");
    } else {
        prompt_and_store_api_key(project_root)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_api_key_accepts_sk_prefix() {
        assert_eq!(
            validate_api_key("  sk-test-key  ").expect("valid"),
            "sk-test-key"
        );
    }

    #[test]
    fn validate_api_key_rejects_empty_or_short_value() {
        assert!(validate_api_key("").is_err());
        assert!(validate_api_key("abc").is_err());
        assert!(validate_api_key("zai-valid-key").is_ok());
    }

    #[test]
    fn preview_key_is_char_safe() {
        assert_eq!(preview_key("密钥abcdefghi"), "密钥abcdef");
    }
}
