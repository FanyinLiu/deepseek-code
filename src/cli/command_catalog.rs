use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cli::resolve_project_root;

#[derive(Debug, Clone)]
pub enum CommandCatalogCommand {
    List { json: bool, filter: Option<String> },
    Locations { json: bool },
}

#[derive(Debug, Serialize)]
struct CommandCatalogPayload {
    project_root: String,
    builtins: Vec<BuiltinCommandPayload>,
    custom: Vec<CustomCommandPayload>,
    skills: Vec<SkillCommandPayload>,
    mcp_prompts: Vec<McpPromptPayload>,
    conflicts: Vec<CommandConflictPayload>,
    locations: Vec<CommandLocationPayload>,
}

#[derive(Debug, Serialize)]
struct BuiltinCommandPayload {
    name: String,
    aliases: Vec<String>,
    group: &'static str,
    description: String,
    usage: String,
}

#[derive(Debug, Serialize)]
struct CustomCommandPayload {
    name: String,
    description: Option<String>,
    source: &'static str,
    path: String,
    conflicts_builtin: bool,
}

#[derive(Debug, Serialize)]
struct SkillCommandPayload {
    name: String,
    description: Option<String>,
    source: &'static str,
    path: String,
}

#[derive(Debug, Serialize)]
struct McpPromptPayload {
    server: String,
    name: String,
    source: String,
    description: String,
    available: bool,
}

#[derive(Debug, Serialize)]
struct CommandConflictPayload {
    name: String,
    builtin: bool,
    custom_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CommandLocationPayload {
    source: &'static str,
    path: String,
    exists: bool,
}

pub async fn command_catalog(
    command: CommandCatalogCommand,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "commands")?;
    match command {
        CommandCatalogCommand::List { json, filter } => list(&root, json, filter.as_deref()),
        CommandCatalogCommand::Locations { json } => locations(&root, json),
    }
}

fn list(root: &Path, json: bool, filter: Option<&str>) -> Result<(), anyhow::Error> {
    let payload = catalog_payload(root, filter)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_catalog(&payload, filter);
    }
    Ok(())
}

fn locations(root: &Path, json: bool) -> Result<(), anyhow::Error> {
    let locations = command_locations(root);
    if json {
        println!("{}", serde_json::to_string_pretty(&locations)?);
    } else {
        println!("Command locations:");
        for location in locations {
            println!(
                "  {:7} {} {}",
                location.source,
                if location.exists { "✓" } else { "○" },
                location.path
            );
        }
    }
    Ok(())
}

fn catalog_payload(
    root: &Path,
    filter: Option<&str>,
) -> Result<CommandCatalogPayload, anyhow::Error> {
    let filter = filter.map(normalize_filter);
    let registry = crate::commands::CommandRegistry::new();
    let mut builtins = registry
        .list_commands()
        .into_iter()
        .map(|cmd| BuiltinCommandPayload {
            name: cmd.name.to_string(),
            aliases: cmd
                .aliases
                .iter()
                .map(|alias| (*alias).to_string())
                .collect(),
            group: command_group(cmd.name),
            description: cmd.description.to_string(),
            usage: cmd.usage.to_string(),
        })
        .filter(|cmd| command_matches_filter(cmd, filter.as_deref()))
        .collect::<Vec<_>>();
    builtins.sort_by(|a, b| a.name.cmp(&b.name));

    let builtin_names = crate::commands::CommandRegistry::new()
        .list_commands()
        .into_iter()
        .flat_map(|cmd| {
            std::iter::once(cmd.name.to_string()).chain(
                cmd.aliases
                    .iter()
                    .map(|alias| (*alias).to_string())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<std::collections::HashSet<_>>();

    let mut custom = Vec::new();
    for location in command_locations(root) {
        if !location.exists {
            continue;
        }
        collect_custom_commands(
            Path::new(&location.path),
            location.source,
            &builtin_names,
            &mut custom,
        )?;
    }
    custom.retain(|cmd| custom_matches_filter(cmd, filter.as_deref()));
    custom.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));

    let mut skills = collect_skills(root)?;
    skills.retain(|skill| skill_matches_filter(skill, filter.as_deref()));
    skills.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));

    let config = crate::storage::Config::load(Some(root))?;
    let mut mcp_prompts = mcp_prompt_namespaces(&config);
    mcp_prompts.retain(|prompt| mcp_prompt_matches_filter(prompt, filter.as_deref()));

    let conflicts = command_conflicts(&builtin_names, &custom);

    Ok(CommandCatalogPayload {
        project_root: root.display().to_string(),
        builtins,
        custom,
        skills,
        mcp_prompts,
        conflicts,
        locations: command_locations(root),
    })
}

fn print_catalog(payload: &CommandCatalogPayload, filter: Option<&str>) {
    match filter {
        Some(value) => println!("Commands matching \"{value}\""),
        None => println!("Commands"),
    }
    println!("Built-in slash commands ({})", payload.builtins.len());
    for command in &payload.builtins {
        let aliases = if command.aliases.is_empty() {
            String::new()
        } else {
            format!(" aliases:{}", command.aliases.join(","))
        };
        println!(
            "  {:18} {:10} {}{}",
            command.name, command.group, command.description, aliases
        );
    }
    println!();
    println!("Custom command files ({})", payload.custom.len());
    if payload.custom.is_empty() {
        println!("  none found");
    } else {
        for command in &payload.custom {
            let conflict = if command.conflicts_builtin {
                " conflict:builtin"
            } else {
                ""
            };
            println!(
                "  {:18} {:7} {}{}",
                command.name,
                command.source,
                command.description.as_deref().unwrap_or("no description"),
                conflict
            );
        }
    }
    println!();
    println!("Skills ({})", payload.skills.len());
    if payload.skills.is_empty() {
        println!("  none found");
    } else {
        for skill in &payload.skills {
            println!(
                "  {:18} {:7} {}",
                skill.name,
                skill.source,
                skill.description.as_deref().unwrap_or("no description")
            );
        }
    }
    println!();
    println!("MCP prompt namespaces ({})", payload.mcp_prompts.len());
    if payload.mcp_prompts.is_empty() {
        println!("  none configured");
    } else {
        for prompt in &payload.mcp_prompts {
            println!(
                "  {:18} {:7} {}",
                prompt.name,
                if prompt.available { "ready" } else { "planned" },
                prompt.description
            );
        }
    }
    println!();
    if !payload.conflicts.is_empty() {
        println!("Conflicts ({})", payload.conflicts.len());
        for conflict in &payload.conflicts {
            println!(
                "  {:18} builtin:{} custom:{}",
                conflict.name,
                conflict.builtin,
                conflict.custom_paths.len()
            );
        }
        println!();
    }
    println!();
    println!("Locations:");
    for location in &payload.locations {
        println!(
            "  {:7} {} {}",
            location.source,
            if location.exists { "✓" } else { "○" },
            location.path
        );
    }
}

fn command_locations(root: &Path) -> Vec<CommandLocationPayload> {
    let project = root.join(".deepseek-code").join("commands");
    let user = dirs::home_dir()
        .map(|home| home.join(".deepseek-code").join("commands"))
        .unwrap_or_else(|| PathBuf::from("~/.deepseek-code/commands"));
    vec![location("project", project), location("user", user)]
}

fn location(source: &'static str, path: PathBuf) -> CommandLocationPayload {
    CommandLocationPayload {
        source,
        exists: path.is_dir(),
        path: path.display().to_string(),
    }
}

fn collect_custom_commands(
    dir: &Path,
    source: &'static str,
    builtin_names: &std::collections::HashSet<String>,
    out: &mut Vec<CustomCommandPayload>,
) -> Result<(), anyhow::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_custom_commands(&path, source, builtin_names, out)?;
            continue;
        }
        if !file_type.is_file() || !is_command_file(&path) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let name = format!("/{}", stem.trim_start_matches('/'));
        let description = read_custom_description(&path).ok().flatten();
        out.push(CustomCommandPayload {
            conflicts_builtin: builtin_names.contains(&name),
            name,
            description,
            source,
            path: path.display().to_string(),
        });
    }
    Ok(())
}

fn collect_skills(root: &Path) -> Result<Vec<SkillCommandPayload>, anyhow::Error> {
    let mut skills = Vec::new();
    for (source, dir) in skill_locations(root) {
        if !dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let skill_dir = entry.path();
            let skill_file = skill_dir.join("SKILL.md");
            if !skill_file.is_file() {
                continue;
            }
            let Some(name) = skill_dir.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            skills.push(SkillCommandPayload {
                name: format!("/skill:{name}"),
                description: read_custom_description(&skill_file).ok().flatten(),
                source,
                path: skill_file.display().to_string(),
            });
        }
    }
    Ok(skills)
}

fn skill_locations(root: &Path) -> Vec<(&'static str, PathBuf)> {
    let project = root.join(".deepseek-code").join("skills");
    let user = dirs::home_dir()
        .map(|home| home.join(".deepseek-code").join("skills"))
        .unwrap_or_else(|| PathBuf::from("~/.deepseek-code/skills"));
    vec![("project", project), ("user", user)]
}

fn mcp_prompt_namespaces(config: &crate::storage::Config) -> Vec<McpPromptPayload> {
    if !config.mcp.enabled {
        return Vec::new();
    }
    config
        .mcp
        .servers
        .keys()
        .map(|server| McpPromptPayload {
            server: server.clone(),
            name: format!("mcp:{server}:prompts"),
            source: format!("mcp:{server}"),
            description: "Prompt discovery namespace; live prompts/list execution is deferred"
                .to_string(),
            available: false,
        })
        .collect()
}

fn command_conflicts(
    builtin_names: &std::collections::HashSet<String>,
    custom: &[CustomCommandPayload],
) -> Vec<CommandConflictPayload> {
    let mut grouped: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for command in custom {
        grouped
            .entry(command.name.clone())
            .or_default()
            .push(command.path.clone());
    }
    grouped
        .into_iter()
        .filter_map(|(name, paths)| {
            let builtin = builtin_names.contains(&name);
            if builtin || paths.len() > 1 {
                Some(CommandConflictPayload {
                    name,
                    builtin,
                    custom_paths: paths,
                })
            } else {
                None
            }
        })
        .collect()
}

fn is_command_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("md" | "markdown" | "toml")
    )
}

fn read_custom_description(path: &Path) -> Result<Option<String>, anyhow::Error> {
    let content = std::fs::read_to_string(path)?;
    if matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("toml")
    ) {
        return Ok(description_from_toml(&content));
    }
    Ok(description_from_markdown(&content))
}

fn description_from_toml(content: &str) -> Option<String> {
    let value = content.parse::<toml::Value>().ok()?;
    value
        .get("description")
        .and_then(toml::Value::as_str)
        .map(trim_description)
        .filter(|value| !value.is_empty())
}

fn description_from_markdown(content: &str) -> Option<String> {
    if let Some(frontmatter) = markdown_frontmatter(content) {
        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("description:") {
                let description = trim_description(value);
                if !description.is_empty() {
                    return Some(description);
                }
            }
        }
    }
    for line in content.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("# ") {
            let description = trim_description(value);
            if !description.is_empty() {
                return Some(description);
            }
        }
    }
    None
}

fn markdown_frontmatter(content: &str) -> Option<&str> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

fn trim_description(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn command_matches_filter(command: &BuiltinCommandPayload, filter: Option<&str>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    command.name.to_lowercase().contains(filter)
        || command.description.to_lowercase().contains(filter)
        || command
            .aliases
            .iter()
            .any(|alias| alias.to_lowercase().contains(filter))
}

fn custom_matches_filter(command: &CustomCommandPayload, filter: Option<&str>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    command.name.to_lowercase().contains(filter)
        || command
            .description
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(filter)
}

fn skill_matches_filter(skill: &SkillCommandPayload, filter: Option<&str>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    skill.name.to_lowercase().contains(filter)
        || skill
            .description
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(filter)
}

fn mcp_prompt_matches_filter(prompt: &McpPromptPayload, filter: Option<&str>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    prompt.name.to_lowercase().contains(filter)
        || prompt.server.to_lowercase().contains(filter)
        || prompt.description.to_lowercase().contains(filter)
}

fn normalize_filter(value: &str) -> String {
    value.trim().to_lowercase()
}

fn command_group(name: &str) -> &'static str {
    match name {
        "/ask" | "/run" | "/plan" | "/review" | "/security-review" | "/simplify" | "/fix"
        | "/explain" | "/wiki" | "/readiness-report" => "agent",
        "/agents" | "/swarm" | "/tasks" | "/schedule" => "team",
        "/mcp" | "/tools" | "/skills" | "/hooks" | "/plugins" | "/commands" => "extension",
        "/permissions" | "/auto" | "/yolo" | "/mode" | "/doctor" => "control",
        "/model" | "/theme" | "/tui" | "/settings" | "/statusline" | "/config" => "settings",
        "/sessions" | "/checkpoint" | "/restore" | "/memory" | "/compact" => "history",
        "/status" | "/context" | "/usage" | "/cwd" | "/search" => "status",
        "/clear" | "/exit" | "/copy" | "/undo" | "/image" | "/commit" | "/test" | "/help" => {
            "local"
        }
        _ => "other",
    }
}
