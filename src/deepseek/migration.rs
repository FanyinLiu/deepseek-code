use super::models::DeepSeekModel;

/// Map legacy model names to current canonical names.
/// Returns None if the name is unrecognized.
/// Old aliases `deepseek-chat` and `deepseek-reasoner` will stop
/// working on 2026-07-24 per official `DeepSeek` announcement.
pub fn migrate_model_name(name: &str) -> Option<DeepSeekModel> {
    match name {
        "deepseek-chat" => {
            tracing::warn!(
                "legacy model alias 'deepseek-chat' used — migrating to deepseek-v4-flash. \
                 This alias will be removed after 2026-07-24."
            );
            Some(DeepSeekModel::Flash)
        }
        "deepseek-reasoner" => {
            tracing::warn!(
                "legacy model alias 'deepseek-reasoner' used — migrating to deepseek-v4-pro. \
                 This alias will be removed after 2026-07-24."
            );
            Some(DeepSeekModel::Pro)
        }
        "deepseek-v4-pro" => Some(DeepSeekModel::Pro),
        "deepseek-v4-flash" => Some(DeepSeekModel::Flash),
        _ => None,
    }
}

/// Check if a model name string is a legacy alias.
#[must_use]
pub fn is_legacy_alias(name: &str) -> bool {
    matches!(name, "deepseek-chat" | "deepseek-reasoner")
}

/// List all valid model names (current + legacy for migration).
#[must_use]
pub fn all_valid_names() -> Vec<&'static str> {
    vec![
        "deepseek-v4-pro",
        "deepseek-v4-flash",
        "deepseek-chat",     // legacy
        "deepseek-reasoner", // legacy
    ]
}
