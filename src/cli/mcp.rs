use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cli::resolve_project_root;
use crate::mcp::client::{McpClient, McpServerConfig, McpTransport};
use crate::mcp::protocol::{
    CallToolResult, McpResource, McpTool, ReadResourceResult, ResourceContent, ToolContent,
};
use crate::storage::{self, config::McpServerEntryConfig};

#[derive(Debug, Clone)]
pub enum McpCommand {
    List {
        json: bool,
    },
    Get {
        name: String,
        json: bool,
    },
    Status {
        json: bool,
        connect: bool,
    },
    ToolsList {
        server: String,
        json: bool,
    },
    ToolsCall {
        server: String,
        tool: String,
        arguments: String,
        json: bool,
    },
    ResourcesList {
        server: String,
        json: bool,
    },
    ResourcesRead {
        server: String,
        uri: String,
        json: bool,
    },
    Add(McpAddArgs),
    Remove {
        name: String,
    },
}

#[derive(Debug, Clone)]
pub struct McpAddArgs {
    pub name: String,
    pub transport: McpTransport,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub env: Vec<String>,
    pub headers: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub trust: bool,
}

#[derive(Debug, Serialize)]
struct McpListPayload {
    enabled: bool,
    servers: Vec<McpServerPayload>,
}

#[derive(Debug, Serialize)]
struct McpServerPayload {
    name: String,
    transport: String,
    command: Option<String>,
    args: Vec<String>,
    url: Option<String>,
    tools_filter: ToolFilterPayload,
    trust: bool,
    timeout_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    connected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ToolFilterPayload {
    include: Vec<String>,
    exclude: Vec<String>,
}

#[derive(Debug, Serialize)]
struct McpStatusPayload {
    enabled: bool,
    connected: bool,
    status: String,
    servers: Vec<McpServerPayload>,
}

#[derive(Debug, Serialize)]
struct McpToolsPayload {
    server: String,
    tools: Vec<McpTool>,
}

#[derive(Debug, Serialize)]
struct McpToolCallPayload {
    server: String,
    tool: String,
    result: CallToolResult,
}

#[derive(Debug, Serialize)]
struct McpResourcesPayload {
    server: String,
    resources: Vec<McpResource>,
}

#[derive(Debug, Serialize)]
struct McpResourceReadPayload {
    server: String,
    uri: String,
    result: ReadResourceResult,
}

pub async fn mcp(command: McpCommand, project_root: Option<PathBuf>) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "mcp")?;
    match command {
        McpCommand::List { json } => list(&root, json),
        McpCommand::Get { name, json } => get(&root, &name, json),
        McpCommand::Status { json, connect } => status(&root, json, connect).await,
        McpCommand::ToolsList { server, json } => list_tools(&root, &server, json).await,
        McpCommand::ToolsCall {
            server,
            tool,
            arguments,
            json,
        } => call_tool(&root, &server, &tool, &arguments, json).await,
        McpCommand::ResourcesList { server, json } => list_resources(&root, &server, json).await,
        McpCommand::ResourcesRead { server, uri, json } => {
            read_resource(&root, &server, &uri, json).await
        }
        McpCommand::Add(args) => add(&root, args),
        McpCommand::Remove { name } => remove(&root, &name),
    }
}

fn list(root: &Path, json: bool) -> Result<(), anyhow::Error> {
    let config = storage::Config::load(Some(root))?;
    let payload = list_payload(&config.mcp);
    if json {
        print_json(&payload)?;
    } else {
        print_list(&payload);
    }
    Ok(())
}

fn get(root: &Path, name: &str, json: bool) -> Result<(), anyhow::Error> {
    let config = storage::Config::load(Some(root))?;
    let Some(server) = config.mcp.servers.get(name) else {
        anyhow::bail!("MCP server not found: {name}");
    };
    let payload = server_payload(name, server);
    if json {
        print_json(&payload)?;
    } else {
        print_server(&payload);
    }
    Ok(())
}

async fn status(root: &Path, json: bool, connect: bool) -> Result<(), anyhow::Error> {
    let config = storage::Config::load(Some(root))?;
    let mut registry = crate::mcp::McpRegistry::with_project_root(root.to_path_buf());
    for (name, server) in &config.mcp.servers {
        registry.register_config(name.clone(), server);
    }
    if config.mcp.enabled && connect {
        registry.connect_all().await;
    }
    let mut servers = list_payload(&config.mcp).servers;
    let runtime = registry
        .server_statuses()
        .into_iter()
        .map(|status| (status.name.clone(), status))
        .collect::<HashMap<_, _>>();
    for server in &mut servers {
        if let Some(status) = runtime.get(&server.name) {
            server.connected = Some(status.connected);
            server.tool_count = Some(status.tool_count);
            server.last_error = status.last_error.clone();
        }
    }
    let payload = McpStatusPayload {
        enabled: config.mcp.enabled,
        connected: registry.has_connected(),
        status: registry.status(),
        servers,
    };
    if json {
        print_json(&payload)?;
    } else {
        println!(
            "MCP: {}",
            if payload.enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
        if connect {
            println!("{}", payload.status);
        } else {
            print_list(&McpListPayload {
                enabled: payload.enabled,
                servers: payload.servers,
            });
            println!("Live check skipped. Use `octo mcp status --connect` to connect.");
        }
    }
    Ok(())
}

async fn list_tools(root: &Path, server: &str, json: bool) -> Result<(), anyhow::Error> {
    let mut client = connect_named_client(root, server).await?;
    let tools = client.list_tools().await?;
    let _ = client.shutdown().await;
    let payload = McpToolsPayload {
        server: server.to_string(),
        tools,
    };
    if json {
        print_json(&payload)?;
    } else {
        println!("MCP tools: {}", payload.server);
        if payload.tools.is_empty() {
            println!("  none advertised");
        }
        for tool in &payload.tools {
            println!(
                "  {:24} {}",
                tool.name,
                tool.description.as_deref().unwrap_or("no description")
            );
        }
    }
    Ok(())
}

async fn call_tool(
    root: &Path,
    server: &str,
    tool: &str,
    arguments: &str,
    json: bool,
) -> Result<(), anyhow::Error> {
    let arguments = crate::mcp::registry::parse_mcp_tool_arguments(arguments)?;
    let mut client = connect_named_client(root, server).await?;
    let result = client.call_tool(tool, arguments).await?;
    let _ = client.shutdown().await;
    let is_error = result.is_error;
    let payload = McpToolCallPayload {
        server: server.to_string(),
        tool: tool.to_string(),
        result,
    };
    if json {
        print_json(&payload)?;
    } else {
        print_tool_result(&payload.result);
    }
    if is_error {
        anyhow::bail!("MCP tool {server}.{tool} returned an error");
    }
    Ok(())
}

async fn list_resources(root: &Path, server: &str, json: bool) -> Result<(), anyhow::Error> {
    let mut client = connect_named_client(root, server).await?;
    let resources = client.list_resources().await?;
    let _ = client.shutdown().await;
    let payload = McpResourcesPayload {
        server: server.to_string(),
        resources,
    };
    if json {
        print_json(&payload)?;
    } else {
        println!("MCP resources: {}", payload.server);
        if payload.resources.is_empty() {
            println!("  none advertised");
        }
        for resource in &payload.resources {
            println!(
                "  {:32} {:18} {}",
                resource.uri,
                resource.mime_type.as_deref().unwrap_or("-"),
                resource.description.as_deref().unwrap_or(&resource.name)
            );
        }
    }
    Ok(())
}

async fn read_resource(
    root: &Path,
    server: &str,
    uri: &str,
    json: bool,
) -> Result<(), anyhow::Error> {
    let mut client = connect_named_client(root, server).await?;
    let result = client.read_resource(uri).await?;
    let _ = client.shutdown().await;
    let payload = McpResourceReadPayload {
        server: server.to_string(),
        uri: uri.to_string(),
        result,
    };
    if json {
        print_json(&payload)?;
    } else {
        print_resource_result(&payload.result);
    }
    Ok(())
}

async fn connect_named_client(root: &Path, server: &str) -> Result<McpClient, anyhow::Error> {
    let config = storage::Config::load(Some(root))?;
    if !config.mcp.enabled {
        anyhow::bail!("MCP is disabled; enable it with `octo mcp add` or config mcp.enabled");
    }
    let Some(entry) = config.mcp.servers.get(server) else {
        anyhow::bail!("MCP server not found: {server}");
    };
    let client_config = McpServerConfig {
        name: server.to_string(),
        transport: entry.transport,
        command: entry.command.clone(),
        args: entry.args.clone(),
        env: entry.env.clone(),
        cwd: Some(root.to_path_buf()),
        url: entry.url.clone(),
        headers: entry.headers.clone(),
    };
    McpClient::connect_with_timeout(&client_config, entry.timeout_ms).await
}

fn print_tool_result(result: &CallToolResult) {
    for content in &result.content {
        match content {
            ToolContent::Text { text } => println!("{text}"),
            ToolContent::Image { data, mime_type } => {
                println!("[image {mime_type}, {} bytes]", data.len());
            }
        }
    }
}

fn print_resource_result(result: &ReadResourceResult) {
    for content in &result.contents {
        match content {
            ResourceContent::Text {
                uri,
                mime_type,
                text,
            } => {
                println!("# {uri} ({})", mime_type.as_deref().unwrap_or("text/plain"));
                println!("{text}");
            }
            ResourceContent::Blob {
                uri,
                mime_type,
                blob,
            } => {
                println!(
                    "# {uri} ({})",
                    mime_type.as_deref().unwrap_or("application/octet-stream")
                );
                println!("[blob {} bytes base64]", blob.len());
            }
        }
    }
}

fn add(root: &Path, args: McpAddArgs) -> Result<(), anyhow::Error> {
    validate_add_args(&args)?;
    let path = local_config_path(root);
    let mut value = read_toml_value(&path)?;
    let server = server_table_mut(&mut value, &args.name);
    server.insert(
        "transport".to_string(),
        toml::Value::String(transport_label(args.transport).to_string()),
    );
    if let Some(command) = args.command {
        server.insert("command".to_string(), toml::Value::String(command));
    }
    if !args.args.is_empty() {
        server.insert("args".to_string(), string_array(args.args));
    }
    if let Some(url) = args.url {
        server.insert("url".to_string(), toml::Value::String(url));
    }
    let env = parse_kv_map(args.env)?;
    if !env.is_empty() {
        server.insert("env".to_string(), toml_table(env));
    }
    let headers = parse_kv_map(args.headers)?;
    if !headers.is_empty() {
        server.insert("headers".to_string(), toml_table(headers));
    }
    if let Some(timeout_ms) = args.timeout_ms {
        server.insert(
            "timeout_ms".to_string(),
            toml::Value::Integer(timeout_ms as i64),
        );
    }
    if args.trust {
        server.insert("trust".to_string(), toml::Value::Boolean(true));
    }
    ensure_mcp_enabled(&mut value);
    write_toml_value(&path, &value)?;
    println!("MCP server '{}' saved to {}", args.name, path.display());
    Ok(())
}

fn remove(root: &Path, name: &str) -> Result<(), anyhow::Error> {
    let path = local_config_path(root);
    let mut value = read_toml_value(&path)?;
    let removed = value
        .get_mut("mcp")
        .and_then(toml::Value::as_table_mut)
        .and_then(|mcp| mcp.get_mut("servers"))
        .and_then(toml::Value::as_table_mut)
        .and_then(|servers| servers.remove(name))
        .is_some();
    write_toml_value(&path, &value)?;
    if removed {
        println!("MCP server '{name}' removed from {}", path.display());
    } else {
        println!(
            "MCP server '{name}' was not configured in {}",
            path.display()
        );
    }
    Ok(())
}

fn list_payload(config: &storage::config::McpConfig) -> McpListPayload {
    McpListPayload {
        enabled: config.enabled,
        servers: config
            .servers
            .iter()
            .map(|(name, server)| server_payload(name, server))
            .collect(),
    }
}

fn server_payload(name: &str, server: &McpServerEntryConfig) -> McpServerPayload {
    McpServerPayload {
        name: name.to_string(),
        transport: transport_label(server.transport).to_string(),
        command: server.command.clone(),
        args: server.args.clone(),
        url: server.url.clone(),
        tools_filter: ToolFilterPayload {
            include: server.include_tools.clone(),
            exclude: server.exclude_tools.clone(),
        },
        trust: server.trust,
        timeout_ms: server.timeout_ms,
        connected: None,
        tool_count: None,
        last_error: None,
    }
}

fn print_list(payload: &McpListPayload) {
    println!(
        "MCP: {}",
        if payload.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    if payload.servers.is_empty() {
        println!("No MCP servers configured.");
        return;
    }
    for server in &payload.servers {
        print_server(server);
    }
}

fn print_server(server: &McpServerPayload) {
    let target = server
        .url
        .as_deref()
        .or(server.command.as_deref())
        .unwrap_or("-");
    let mut line = format!(
        "  {} | {} | {} | timeout={}ms | trust={}",
        server.name, server.transport, target, server.timeout_ms, server.trust
    );
    if let Some(connected) = server.connected {
        line.push_str(&format!(
            " | connected={} | tools={}",
            connected,
            server.tool_count.unwrap_or(0)
        ));
    }
    if let Some(error) = &server.last_error {
        line.push_str(&format!(" | error={error}"));
    }
    println!("{line}");
    if !server.tools_filter.include.is_empty() {
        println!(
            "    include_tools={}",
            server.tools_filter.include.join(", ")
        );
    }
    if !server.tools_filter.exclude.is_empty() {
        println!(
            "    exclude_tools={}",
            server.tools_filter.exclude.join(", ")
        );
    }
}

fn validate_add_args(args: &McpAddArgs) -> Result<(), anyhow::Error> {
    match args.transport {
        McpTransport::Stdio if args.command.is_none() => {
            anyhow::bail!("stdio MCP server requires --command")
        }
        McpTransport::Http | McpTransport::Sse if args.url.is_none() => {
            anyhow::bail!("remote MCP server requires --url")
        }
        _ => Ok(()),
    }
}

fn transport_label(transport: McpTransport) -> &'static str {
    match transport {
        McpTransport::Stdio => "stdio",
        McpTransport::Http => "http",
        McpTransport::Sse => "sse",
    }
}

fn local_config_path(root: &Path) -> PathBuf {
    root.join(".octocode").join("local.toml")
}

fn read_toml_value(path: &Path) -> Result<toml::Value, anyhow::Error> {
    if path.exists() {
        let text = crate::storage::read_text_file_capped(path)?;
        text.parse::<toml::Value>().map_err(|e| {
            anyhow::anyhow!(
                "mcp config file {} is corrupted: {}. Refusing to overwrite; fix or remove it first.",
                path.display(),
                e
            )
        })
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

fn ensure_mcp_enabled(value: &mut toml::Value) {
    table_mut(value, "mcp").insert("enabled".to_string(), toml::Value::Boolean(true));
}

fn server_table_mut<'a>(
    value: &'a mut toml::Value,
    name: &str,
) -> &'a mut toml::map::Map<String, toml::Value> {
    let mcp = table_mut(value, "mcp");
    let servers = mcp
        .entry("servers".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    if !servers.is_table() {
        *servers = toml::Value::Table(toml::map::Map::new());
    }
    let servers = servers.as_table_mut().expect("servers table");
    let server = servers
        .entry(name.to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    if !server.is_table() {
        *server = toml::Value::Table(toml::map::Map::new());
    }
    server.as_table_mut().expect("server table")
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

fn parse_kv_map(values: Vec<String>) -> Result<HashMap<String, String>, anyhow::Error> {
    let mut map = HashMap::new();
    for value in values {
        let Some((key, val)) = value.split_once('=') else {
            anyhow::bail!("expected KEY=VALUE, got '{value}'");
        };
        map.insert(key.to_string(), val.to_string());
    }
    Ok(map)
}

fn toml_table(values: HashMap<String, String>) -> toml::Value {
    toml::Value::Table(
        values
            .into_iter()
            .map(|(key, value)| (key, toml::Value::String(value)))
            .collect(),
    )
}

fn string_array(values: Vec<String>) -> toml::Value {
    toml::Value::Array(values.into_iter().map(toml::Value::String).collect())
}

fn print_json<T: Serialize>(value: &T) -> Result<(), anyhow::Error> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
