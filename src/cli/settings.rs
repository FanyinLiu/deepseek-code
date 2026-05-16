use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cli::resolve_project_root;
use crate::storage;

#[derive(Debug, Clone)]
pub enum SettingsCommand {
    List { json: bool },
    Get { key: String },
    Set { key: String, value: String },
}

#[derive(Debug, Serialize)]
struct SettingItem {
    key: &'static str,
    value: String,
    writable: bool,
}

pub async fn settings(
    command: SettingsCommand,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "settings")?;
    match command {
        SettingsCommand::List { json } => list(&root, json),
        SettingsCommand::Get { key } => get(&root, &key),
        SettingsCommand::Set { key, value } => set(&root, &key, &value),
    }
}

fn list(root: &Path, json: bool) -> Result<(), anyhow::Error> {
    let config = storage::Config::load(Some(root))?;
    let items = setting_items(&config);
    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        for item in &items {
            println!("{:36} {}", item.key, item.value);
        }
    }
    Ok(())
}

fn get(root: &Path, key: &str) -> Result<(), anyhow::Error> {
    let config = storage::Config::load(Some(root))?;
    let item = setting_items(&config)
        .into_iter()
        .find(|item| item.key == key)
        .ok_or_else(|| anyhow::anyhow!("unknown setting key '{key}'"))?;
    println!("{}", item.value);
    Ok(())
}

fn set(root: &Path, key: &str, value: &str) -> Result<(), anyhow::Error> {
    let toml_value = validate_setting_value(key, value)?;
    let path = local_config_path(root);
    let mut config = read_toml_value(&path)?;
    set_toml_path(&mut config, key, toml_value)?;
    write_toml_value(&path, &config)?;
    println!("{key} saved to {}", path.display());
    Ok(())
}

fn setting_items(config: &storage::Config) -> Vec<SettingItem> {
    vec![
        item("model.default", config.model.default.to_string()),
        item("model.heavy", config.model.heavy.to_string()),
        item(
            "model.thinking_mode",
            config.model.thinking_mode.to_string(),
        ),
        item(
            "model.reasoning_effort",
            config.model.reasoning_effort.to_string(),
        ),
        item("provider.default", config.provider.default.as_str()),
        item("ui.theme", &config.ui.theme),
        item(
            "ui.show_reasoning_summary",
            config.ui.show_reasoning_summary.to_string(),
        ),
        item(
            "ui.show_raw_reasoning",
            config.ui.show_raw_reasoning.to_string(),
        ),
        item("ui.show_cache_hud", config.ui.show_cache_hud.to_string()),
        item(
            "policy.autonomy_level",
            config.policy.autonomy_level.as_str(),
        ),
        item(
            "policy.require_approval_for_write",
            config.policy.require_approval_for_write.to_string(),
        ),
        item(
            "policy.require_approval_for_command",
            config.policy.require_approval_for_command.to_string(),
        ),
        item(
            "policy.network_access",
            config.policy.network_access.to_string(),
        ),
        item(
            "policy.command_sandbox.mode",
            config.policy.command_sandbox.mode.as_str(),
        ),
        item("mcp.enabled", config.mcp.enabled.to_string()),
        item(
            "subagent.max_parallel",
            config.subagent.max_parallel.to_string(),
        ),
    ]
}

fn item(key: &'static str, value: impl ToString) -> SettingItem {
    SettingItem {
        key,
        value: value.to_string(),
        writable: true,
    }
}

fn validate_setting_value(key: &str, value: &str) -> Result<toml::Value, anyhow::Error> {
    match key {
        "model.default" | "model.heavy" => {
            crate::provider::parse_model(value)?;
            Ok(toml::Value::String(value.to_string()))
        }
        "model.thinking_mode" => match value {
            "on" | "off" | "auto" => Ok(toml::Value::String(value.to_string())),
            _ => anyhow::bail!("model.thinking_mode must be on, off, or auto"),
        },
        "model.reasoning_effort" => match value {
            "min" | "low" | "medium" | "high" | "max" => Ok(toml::Value::String(value.to_string())),
            _ => anyhow::bail!("model.reasoning_effort must be min, low, medium, high, or max"),
        },
        "provider.default" => match value {
            "deepseek" | "openrouter" | "openai-compatible" => {
                Ok(toml::Value::String(value.to_string()))
            }
            _ => {
                anyhow::bail!("provider.default must be deepseek, openrouter, or openai-compatible")
            }
        },
        "ui.theme" => match value {
            "auto" | "light" | "dark" | "high-contrast" => {
                Ok(toml::Value::String(value.to_string()))
            }
            _ => anyhow::bail!("ui.theme must be auto, light, dark, or high-contrast"),
        },
        "ui.show_reasoning_summary"
        | "ui.show_raw_reasoning"
        | "ui.show_cache_hud"
        | "policy.require_approval_for_write"
        | "policy.require_approval_for_command"
        | "policy.network_access"
        | "mcp.enabled" => Ok(toml::Value::Boolean(parse_bool(value)?)),
        "policy.autonomy_level" => match value {
            "off" | "low" | "medium" | "high" => Ok(toml::Value::String(value.to_string())),
            _ => anyhow::bail!("policy.autonomy_level must be off, low, medium, or high"),
        },
        "policy.command_sandbox.mode" => match value {
            "off" | "auto" | "strict" => Ok(toml::Value::String(value.to_string())),
            _ => anyhow::bail!("policy.command_sandbox.mode must be off, auto, or strict"),
        },
        "subagent.max_parallel" => {
            let parsed = value.parse::<i64>()?;
            if parsed < 1 {
                anyhow::bail!("subagent.max_parallel must be at least 1");
            }
            Ok(toml::Value::Integer(parsed))
        }
        _ => anyhow::bail!("unknown setting key '{key}'"),
    }
}

fn parse_bool(value: &str) -> Result<bool, anyhow::Error> {
    match value {
        "true" | "on" | "1" | "yes" => Ok(true),
        "false" | "off" | "0" | "no" => Ok(false),
        _ => anyhow::bail!("expected boolean value"),
    }
}

fn local_config_path(root: &Path) -> PathBuf {
    root.join(".deepseek-code").join("local.toml")
}

fn read_toml_value(path: &Path) -> Result<toml::Value, anyhow::Error> {
    if path.exists() {
        Ok(std::fs::read_to_string(path)?
            .parse::<toml::Value>()
            .unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new())))
    } else {
        Ok(toml::Value::Table(toml::map::Map::new()))
    }
}

fn write_toml_value(path: &Path, value: &toml::Value) -> Result<(), anyhow::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(value)?)?;
    Ok(())
}

fn table_mut<'a>(
    value: &'a mut toml::Value,
    name: &str,
) -> &'a mut toml::map::Map<String, toml::Value> {
    if !value.is_table() {
        *value = toml::Value::Table(toml::map::Map::new());
    }
    let root = value.as_table_mut().expect("root table");
    let child = root
        .entry(name.to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    if !child.is_table() {
        *child = toml::Value::Table(toml::map::Map::new());
    }
    child.as_table_mut().expect("child table")
}

fn set_toml_path(
    value: &mut toml::Value,
    key: &str,
    new_value: toml::Value,
) -> Result<(), anyhow::Error> {
    let mut parts: Vec<&str> = key.split('.').collect();
    let field = parts
        .pop()
        .ok_or_else(|| anyhow::anyhow!("setting key must not be empty"))?;
    if parts.is_empty() {
        anyhow::bail!("setting key must be table.field");
    }
    let mut table = table_mut(value, parts[0]);
    for part in &parts[1..] {
        let child = table
            .entry((*part).to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if !child.is_table() {
            *child = toml::Value::Table(toml::map::Map::new());
        }
        table = child.as_table_mut().expect("nested table");
    }
    table.insert(field.to_string(), new_value);
    Ok(())
}
