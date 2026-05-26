use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Serialize;

use crate::cli::{resolve_project_root, TurnOutputFormat};

#[derive(Debug, Clone)]
pub enum CommandCatalogCommand {
    List {
        json: bool,
        filter: Option<String>,
    },
    Show {
        name: String,
        json: bool,
    },
    Run {
        name: String,
        args: Vec<String>,
        dry_run: bool,
        json: bool,
        thinking: bool,
        output_format: TurnOutputFormat,
    },
    Locations {
        json: bool,
    },
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
    argument_hint: Option<String>,
    model: Option<String>,
    allowed_tools: Option<Vec<String>>,
    conflicts_builtin: bool,
}

#[derive(Debug, Serialize)]
struct CustomCommandRunPayload {
    name: String,
    description: Option<String>,
    source: &'static str,
    path: String,
    argument_hint: Option<String>,
    model: Option<String>,
    allowed_tools: Option<Vec<String>>,
    args: Vec<String>,
    prompt: String,
    dry_run: bool,
}

#[derive(Debug, Clone)]
struct CustomCommandDocument {
    name: String,
    description: Option<String>,
    source: &'static str,
    path: PathBuf,
    argument_hint: Option<String>,
    model: Option<String>,
    allowed_tools: Option<Vec<String>>,
    prompt_template: String,
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
        CommandCatalogCommand::Show { name, json } => show(&root, &name, json),
        CommandCatalogCommand::Run {
            name,
            args,
            dry_run,
            json,
            thinking,
            output_format,
        } => run_custom_command(&root, &name, args, dry_run, json, thinking, output_format).await,
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

fn show(root: &Path, name: &str, json: bool) -> Result<(), anyhow::Error> {
    let command = find_custom_command(root, name)?;
    let payload = CustomCommandRunPayload {
        name: command.name.clone(),
        description: command.description.clone(),
        source: command.source,
        path: command.path.display().to_string(),
        argument_hint: command.argument_hint.clone(),
        model: command.model.clone(),
        allowed_tools: command.allowed_tools.clone(),
        args: Vec::new(),
        prompt: command.prompt_template.clone(),
        dry_run: true,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_custom_command(&payload, "Custom command");
    }
    Ok(())
}

async fn run_custom_command(
    root: &Path,
    name: &str,
    args: Vec<String>,
    dry_run: bool,
    json: bool,
    thinking: bool,
    output_format: TurnOutputFormat,
) -> Result<(), anyhow::Error> {
    let command = find_custom_command(root, name)?;
    let raw_prompt = render_custom_command_prompt(&command.prompt_template, &args);
    let bang_enabled = crate::commands::allowed_tools_permit_bang(command.allowed_tools.as_deref());
    let prompt = crate::commands::expand_bang_lines(&raw_prompt, root, bang_enabled);
    let payload = CustomCommandRunPayload {
        name: command.name,
        description: command.description,
        source: command.source,
        path: command.path.display().to_string(),
        argument_hint: command.argument_hint,
        model: command.model.clone(),
        allowed_tools: command.allowed_tools.clone(),
        args,
        prompt,
        dry_run,
    };

    if payload.dry_run {
        if json {
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            print_custom_command(&payload, "Custom command dry run");
        }
        return Ok(());
    }

    let output_format = if json {
        TurnOutputFormat::Json
    } else {
        output_format
    };
    if !output_format.is_json() && !output_format.is_stream_json() {
        println!(
            "Running custom command {} from {}",
            payload.name, payload.path
        );
    }
    crate::cli::run(
        payload.prompt,
        thinking,
        Some(root.to_path_buf()),
        output_format,
        crate::cli::ToolApprovalPolicy::Deny,
        payload.model.clone(),
        payload.allowed_tools.clone(),
    )
    .await
}

fn print_custom_command(payload: &CustomCommandRunPayload, title: &str) {
    println!("{title}: {}", payload.name);
    println!("Source: {} ({})", payload.source, payload.path);
    if let Some(description) = &payload.description {
        println!("Description: {description}");
    }
    if let Some(argument_hint) = &payload.argument_hint {
        println!("Arguments: {argument_hint}");
    }
    if let Some(model) = &payload.model {
        println!("Model: {model}");
    }
    if let Some(tools) = &payload.allowed_tools {
        println!("Allowed tools: {}", tools.join(", "));
    }
    if !payload.args.is_empty() {
        println!("Input args: {}", payload.args.join(" "));
    }
    println!();
    println!("{}", payload.prompt);
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
        let base = PathBuf::from(&location.path);
        collect_custom_commands(&base, &base, location.source, &builtin_names, &mut custom)?;
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
    let project = root.join(".octocode").join("commands");
    let user = crate::storage::user_home_dir()
        .map(|home| home.join(".octocode").join("commands"))
        .unwrap_or_else(|| PathBuf::from("~/.octocode/commands"));
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
    base: &Path,
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
            collect_custom_commands(base, &path, source, builtin_names, out)?;
            continue;
        }
        if !file_type.is_file() || !is_command_file(&path) {
            continue;
        }
        let Some(name) = command_name_for(base, &path) else {
            continue;
        };
        let description = read_custom_description(&path).ok().flatten();
        let argument_hint = read_custom_argument_hint(&path).ok().flatten();
        let model = read_custom_model(&path).ok().flatten();
        let allowed_tools = read_custom_allowed_tools(&path).ok().flatten();
        out.push(CustomCommandPayload {
            conflicts_builtin: builtin_names.contains(&name),
            name,
            description,
            argument_hint,
            model,
            allowed_tools,
            source,
            path: path.display().to_string(),
        });
    }
    Ok(())
}

/// `git/status.md` under `~/.octocode/commands/` becomes `/git:status` for
/// markdown files, matching the TUI loader. `.toml` files are kept as bare
/// `/<stem>` because the legacy TOML loader does not implement namespacing.
fn command_name_for(base: &Path, file: &Path) -> Option<String> {
    let extension = file.extension().and_then(|value| value.to_str());
    if matches!(extension, Some("md")) {
        if let Some(name) = crate::commands::prompt_command_name(base, file) {
            return Some(name);
        }
    }
    let stem = file.file_stem().and_then(|value| value.to_str())?;
    Some(format!("/{}", stem.trim_start_matches('/')))
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
    let project = root.join(".octocode").join("skills");
    let user = crate::storage::user_home_dir()
        .map(|home| home.join(".octocode").join("skills"))
        .unwrap_or_else(|| PathBuf::from("~/.octocode/skills"));
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

fn find_custom_command(root: &Path, name: &str) -> Result<CustomCommandDocument, anyhow::Error> {
    let name = normalize_command_name(name);
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
    if builtin_names.contains(&name) {
        bail!("custom command {name} conflicts with a built-in slash command");
    }

    let mut matches = Vec::new();
    for location in command_locations(root) {
        if !location.exists {
            continue;
        }
        let base = PathBuf::from(&location.path);
        collect_matching_command_documents(&base, &base, location.source, &name, &mut matches)?;
    }
    match matches.len() {
        0 => bail!("unknown custom command {name}"),
        1 => Ok(matches.remove(0)),
        _ => {
            let paths = matches
                .iter()
                .map(|command| command.path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("ambiguous custom command {name}; matches: {paths}")
        }
    }
}

fn collect_matching_command_documents(
    base: &Path,
    dir: &Path,
    source: &'static str,
    name: &str,
    out: &mut Vec<CustomCommandDocument>,
) -> Result<(), anyhow::Error> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_matching_command_documents(base, &path, source, name, out)?;
            continue;
        }
        if !file_type.is_file() || !is_command_file(&path) {
            continue;
        }
        let Some(candidate) = command_name_for(base, &path) else {
            continue;
        };
        if candidate != name {
            continue;
        }
        out.push(read_custom_command_document(&path, source, name)?);
    }
    Ok(())
}

fn read_custom_command_document(
    path: &Path,
    source: &'static str,
    name: &str,
) -> Result<CustomCommandDocument, anyhow::Error> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let parsed = if matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("toml")
    ) {
        custom_command_from_toml(&content)?
    } else {
        custom_command_from_markdown(&content)
    };
    if parsed.prompt_template.trim().is_empty() {
        bail!(
            "custom command {name} has no prompt body: {}",
            path.display()
        );
    }

    Ok(CustomCommandDocument {
        name: name.to_string(),
        description: parsed.description,
        source,
        path: path.to_path_buf(),
        argument_hint: parsed.argument_hint,
        model: parsed.model,
        allowed_tools: parsed.allowed_tools,
        prompt_template: parsed.prompt_template,
    })
}

struct ParsedCustomCommand {
    description: Option<String>,
    argument_hint: Option<String>,
    model: Option<String>,
    allowed_tools: Option<Vec<String>>,
    prompt_template: String,
}

fn custom_command_from_toml(content: &str) -> Result<ParsedCustomCommand, anyhow::Error> {
    let value: toml::Value = content.parse()?;
    let description = value
        .get("description")
        .and_then(toml::Value::as_str)
        .map(trim_description)
        .filter(|value| !value.is_empty());
    let argument_hint = value
        .get("argument_hint")
        .or_else(|| value.get("argument-hint"))
        .and_then(toml::Value::as_str)
        .map(trim_description)
        .filter(|value| !value.is_empty());
    let model = value
        .get("model")
        .and_then(toml::Value::as_str)
        .map(trim_description)
        .filter(|value| !value.is_empty());
    let allowed_tools = allowed_tools_from_toml(&value);
    let prompt = value
        .get("prompt")
        .or_else(|| value.get("template"))
        .or_else(|| value.get("body"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    Ok(ParsedCustomCommand {
        description,
        argument_hint,
        model,
        allowed_tools,
        prompt_template: prompt,
    })
}

fn custom_command_from_markdown(content: &str) -> ParsedCustomCommand {
    ParsedCustomCommand {
        description: description_from_markdown(content),
        argument_hint: argument_hint_from_markdown(content),
        model: model_from_markdown(content),
        allowed_tools: allowed_tools_from_markdown(content),
        prompt_template: markdown_body(content).trim().to_string(),
    }
}

fn allowed_tools_from_toml(value: &toml::Value) -> Option<Vec<String>> {
    let raw = value
        .get("allowed_tools")
        .or_else(|| value.get("allowed-tools"))?;
    if let Some(array) = raw.as_array() {
        let tools: Vec<String> = array
            .iter()
            .filter_map(|item| item.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();
        if tools.is_empty() {
            None
        } else {
            Some(tools)
        }
    } else if let Some(text) = raw.as_str() {
        crate::commands::parse_allowed_tools_value(text)
    } else {
        None
    }
}

fn allowed_tools_from_markdown(content: &str) -> Option<Vec<String>> {
    let frontmatter = markdown_frontmatter(content)?;
    for line in frontmatter.lines() {
        let line = line.trim();
        for key in [
            "allowed-tools:",
            "allowed_tools:",
            "allowed-tools =",
            "allowed_tools =",
        ] {
            if let Some(value) = line.strip_prefix(key) {
                if let Some(tools) = crate::commands::parse_allowed_tools_value(value) {
                    return Some(tools);
                }
            }
        }
    }
    None
}

fn model_from_markdown(content: &str) -> Option<String> {
    let frontmatter = markdown_frontmatter(content)?;
    for line in frontmatter.lines() {
        let line = line.trim();
        for key in ["model:", "model ="] {
            if let Some(value) = line.strip_prefix(key) {
                let model = trim_description(value);
                if !model.is_empty() {
                    return Some(model);
                }
            }
        }
    }
    None
}

fn read_custom_argument_hint(path: &Path) -> Result<Option<String>, anyhow::Error> {
    let content = std::fs::read_to_string(path)?;
    if matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("toml")
    ) {
        let value: toml::Value = content.parse()?;
        return Ok(value
            .get("argument_hint")
            .or_else(|| value.get("argument-hint"))
            .and_then(toml::Value::as_str)
            .map(trim_description)
            .filter(|value| !value.is_empty()));
    }
    Ok(argument_hint_from_markdown(&content))
}

fn read_custom_allowed_tools(path: &Path) -> Result<Option<Vec<String>>, anyhow::Error> {
    let content = std::fs::read_to_string(path)?;
    if matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("toml")
    ) {
        let value: toml::Value = content.parse()?;
        return Ok(allowed_tools_from_toml(&value));
    }
    Ok(allowed_tools_from_markdown(&content))
}

fn read_custom_model(path: &Path) -> Result<Option<String>, anyhow::Error> {
    let content = std::fs::read_to_string(path)?;
    if matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("toml")
    ) {
        let value: toml::Value = content.parse()?;
        return Ok(value
            .get("model")
            .and_then(toml::Value::as_str)
            .map(trim_description)
            .filter(|value| !value.is_empty()));
    }
    Ok(model_from_markdown(&content))
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
            if let Some(value) = line.strip_prefix("description =") {
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

fn argument_hint_from_markdown(content: &str) -> Option<String> {
    let frontmatter = markdown_frontmatter(content)?;
    for line in frontmatter.lines() {
        let line = line.trim();
        for key in [
            "argument-hint:",
            "argument_hint:",
            "argument-hint =",
            "argument_hint =",
        ] {
            if let Some(value) = line.strip_prefix(key) {
                let argument_hint = trim_description(value);
                if !argument_hint.is_empty() {
                    return Some(argument_hint);
                }
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

fn markdown_body(content: &str) -> &str {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let Some(rest) = content.strip_prefix("---\n") else {
        return content;
    };
    let Some(end) = rest.find("\n---") else {
        return content;
    };
    &rest[end + "\n---".len()..]
}

fn render_custom_command_prompt(template: &str, args: &[String]) -> String {
    let arguments = args.join(" ");
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut prompt = crate::commands::apply_positional_args(template, &arg_refs);
    prompt = prompt.replace("$ARGUMENTS", &arguments);
    prompt = prompt.replace("{{args}}", &arguments);
    prompt = prompt.replace("{{arguments}}", &arguments);
    if arguments.is_empty() || prompt.contains(&arguments) {
        prompt.trim().to_string()
    } else {
        format!("{}\n\nArguments:\n{}", prompt.trim(), arguments)
    }
}

fn normalize_command_name(name: &str) -> String {
    let name = name.trim();
    let name = name.strip_suffix(".md").unwrap_or(name);
    let name = name.strip_suffix(".markdown").unwrap_or(name);
    let name = name.strip_suffix(".toml").unwrap_or(name);
    format!("/{}", name.trim_start_matches('/'))
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
        "/ask" | "/btw" | "/run" | "/plan" | "/review" | "/security-review" | "/simplify"
        | "/fix" | "/explain" | "/wiki" | "/readiness-report" => "agent",
        "/agents" | "/swarm" | "/tasks" | "/task" | "/schedule" => "team",
        "/mcp" | "/tools" | "/skills" | "/hooks" | "/plugins" | "/commands" => "extension",
        "/permissions" | "/auto" | "/yolo" | "/mode" | "/doctor" => "control",
        "/model" | "/theme" | "/tui" | "/settings" | "/language" | "/output-style"
        | "/keybindings" | "/statusline" | "/config" => "settings",
        "/sessions" | "/checkpoint" | "/restore" | "/memory" | "/compact" | "/recap" | "/todo" => {
            "history"
        }
        "/status" | "/context" | "/usage" | "/cwd" | "/search" | "/glob" | "/grep" => "status",
        "/clear" | "/exit" | "/copy" | "/undo" | "/image" | "/commit" | "/test" | "/help" => {
            "local"
        }
        _ => "other",
    }
}
