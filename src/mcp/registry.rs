//! MCP server registry — manages multiple MCP connections and aggregates their tools.
use std::{collections::HashMap, path::PathBuf};

use super::client::{McpClient, McpServerConfig, McpTransport, DEFAULT_MCP_TIMEOUT_MS};
use super::protocol::McpTool;

#[derive(Debug, Clone, Default)]
pub struct McpServerOptions {
    pub include_tools: Vec<String>,
    pub exclude_tools: Vec<String>,
    pub trust: bool,
    pub timeout_ms: Option<u64>,
}

impl McpServerOptions {
    #[must_use]
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms.unwrap_or(DEFAULT_MCP_TIMEOUT_MS)
    }

    #[must_use]
    pub fn allows_tool(&self, tool_name: &str) -> bool {
        let included = self.include_tools.is_empty()
            || self
                .include_tools
                .iter()
                .any(|allowed| allowed == tool_name);
        let excluded = self
            .exclude_tools
            .iter()
            .any(|blocked| blocked == tool_name);
        included && !excluded
    }
}

/// A registered MCP server entry.
pub struct McpServerEntry {
    pub config: McpServerConfig,
    pub options: McpServerOptions,
    pub client: Option<McpClient>,
    pub tools: Vec<McpTool>,
    pub connected: bool,
    pub error: Option<String>,
}

/// Registry of MCP servers.
pub struct McpRegistry {
    servers: HashMap<String, McpServerEntry>,
    project_root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolIdentity {
    pub server: String,
    pub tool: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct McpToolApprovalMetadata {
    pub identity: McpToolIdentity,
    pub trust: bool,
    pub include_tools: Vec<String>,
    pub exclude_tools: Vec<String>,
    pub transport: McpTransport,
    pub advertised: bool,
    pub allowed_by_filters: bool,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerRuntimeStatus {
    pub name: String,
    pub connected: bool,
    pub tool_count: usize,
    pub last_error: Option<String>,
}

impl McpRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
            project_root: None,
        }
    }

    #[must_use]
    pub fn with_project_root(project_root: PathBuf) -> Self {
        Self {
            servers: HashMap::new(),
            project_root: Some(project_root),
        }
    }

    /// Register a server config (does not connect yet).
    pub fn register(&mut self, config: McpServerConfig) {
        self.register_with_options(config, McpServerOptions::default());
    }

    /// Register a server config with per-server hardening options.
    pub fn register_with_options(&mut self, config: McpServerConfig, options: McpServerOptions) {
        let name = config.name.clone();
        self.servers.insert(
            name,
            McpServerEntry {
                config,
                options,
                client: None,
                tools: Vec::new(),
                connected: false,
                error: None,
            },
        );
    }

    /// Register a server from the persisted MCP config shape.
    pub fn register_config(
        &mut self,
        name: String,
        server_config: &crate::storage::config::McpServerEntryConfig,
    ) {
        self.register_with_options(
            McpServerConfig {
                name,
                transport: server_config.transport,
                command: server_config.command.clone(),
                args: server_config.args.clone(),
                env: server_config.env.clone(),
                cwd: self.project_root.clone(),
                url: server_config.url.clone(),
                headers: server_config.headers.clone(),
            },
            McpServerOptions {
                include_tools: server_config.include_tools.clone(),
                exclude_tools: server_config.exclude_tools.clone(),
                trust: server_config.trust,
                timeout_ms: Some(server_config.timeout_ms),
            },
        );
    }

    /// Connect all registered servers and discover tools.
    pub async fn connect_all(&mut self) {
        let names: Vec<String> = self.servers.keys().cloned().collect();
        for name in names {
            if let Some(entry) = self.servers.get_mut(&name) {
                match McpClient::connect_with_timeout(&entry.config, entry.options.timeout_ms())
                    .await
                {
                    Ok(mut client) => match client.list_tools().await {
                        Ok(tools) => {
                            entry.tools = filter_tools(tools, &entry.options);
                            entry.connected = true;
                            entry.client = Some(client);
                        }
                        Err(e) => {
                            entry.error = Some(format!("list_tools failed: {e}"));
                        }
                    },
                    Err(e) => {
                        entry.error = Some(e.to_string());
                    }
                }
            }
        }
    }

    /// Get all aggregated tools from all connected servers.
    /// Returns (server_name, tool) pairs.
    #[must_use]
    pub fn all_tools(&self) -> Vec<(&str, &McpTool)> {
        let mut result = Vec::new();
        // Iterate servers in a deterministic (name-sorted) order. HashMap
        // iteration order varies per registry instance, which would reorder the
        // MCP tool block between sessions and break DeepSeek's prefix cache.
        let mut names: Vec<&String> = self.servers.keys().collect();
        names.sort();
        for name in names {
            let entry = &self.servers[name];
            if entry.connected {
                for tool in &entry.tools {
                    result.push((name.as_str(), tool));
                }
            }
        }
        result
    }

    #[must_use]
    pub fn is_mcp_tool_name(&self, server_tool: &str) -> bool {
        parse_server_tool(server_tool)
            .is_some_and(|(server_name, _)| self.servers.contains_key(server_name))
    }

    #[must_use]
    pub fn tool_approval_metadata(&self, server_tool: &str) -> Option<McpToolApprovalMetadata> {
        let (server_name, tool_name) = parse_server_tool(server_tool)?;
        let entry = self.servers.get(server_name)?;
        let advertised_tool = entry.tools.iter().find(|tool| tool.name == tool_name);
        Some(McpToolApprovalMetadata {
            identity: McpToolIdentity {
                server: server_name.to_string(),
                tool: tool_name.to_string(),
                source: format!("mcp:{server_name}.{tool_name}"),
            },
            trust: entry.options.trust,
            include_tools: entry.options.include_tools.clone(),
            exclude_tools: entry.options.exclude_tools.clone(),
            transport: entry.config.transport,
            advertised: advertised_tool.is_some(),
            allowed_by_filters: entry.options.allows_tool(tool_name),
            description: advertised_tool.and_then(|tool| tool.description.clone()),
            input_schema: advertised_tool
                .map(|tool| tool.input_schema.clone())
                .unwrap_or_else(|| serde_json::json!({})),
        })
    }

    /// Call a tool by its server-prefixed name (`server_name.tool_name`).
    pub async fn call_tool(
        &mut self,
        server_tool: &str,
        arguments: serde_json::Value,
    ) -> Result<String, anyhow::Error> {
        let (server_name, tool_name) = parse_server_tool(server_tool).ok_or_else(|| {
            anyhow::anyhow!("tool name must be 'server_name.tool_name', got: {server_tool}")
        })?;

        let entry = self
            .servers
            .get_mut(server_name)
            .ok_or_else(|| anyhow::anyhow!("unknown MCP server: {server_name}"))?;

        if !entry.options.allows_tool(tool_name) {
            anyhow::bail!("MCP tool {server_tool} is blocked by include/exclude filters");
        }

        if !entry.tools.iter().any(|tool| tool.name == tool_name) {
            anyhow::bail!("MCP tool {server_tool} was not advertised by the connected server");
        }

        let client = entry
            .client
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("MCP server {server_name} is not connected"))?;

        let result = client.call_tool(tool_name, arguments).await?;
        let mut output = String::new();
        for content in &result.content {
            match content {
                super::protocol::ToolContent::Text { text } => {
                    output.push_str(text);
                    output.push('\n');
                }
                super::protocol::ToolContent::Image { data, mime_type } => {
                    output.push_str(&format!("[Image: {mime_type}, {} bytes]\n", data.len()));
                }
            }
        }
        if result.is_error {
            anyhow::bail!("Tool returned error: {output}");
        }
        Ok(output)
    }

    /// Check if any servers are connected.
    #[must_use]
    pub fn has_connected(&self) -> bool {
        self.servers.values().any(|e| e.connected)
    }

    /// Get status summary for all servers.
    #[must_use]
    pub fn status(&self) -> String {
        let mut lines = vec!["MCP Servers:".to_string()];
        for (name, entry) in &self.servers {
            let status = if entry.connected {
                format!("  ✅ {name} — {} tools", entry.tools.len())
            } else if let Some(ref err) = entry.error {
                format!("  ❌ {name} — {err}")
            } else {
                format!("  ○ {name} — not connected")
            };
            lines.push(status);
        }
        lines.join("\n")
    }

    #[must_use]
    pub fn server_statuses(&self) -> Vec<McpServerRuntimeStatus> {
        let mut statuses = self
            .servers
            .iter()
            .map(|(name, entry)| McpServerRuntimeStatus {
                name: name.clone(),
                connected: entry.connected,
                tool_count: entry.tools.len(),
                last_error: entry.error.clone(),
            })
            .collect::<Vec<_>>();
        statuses.sort_by(|a, b| a.name.cmp(&b.name));
        statuses
    }

    /// Disconnect all servers gracefully.
    pub async fn disconnect_all(&mut self) {
        for entry in self.servers.values_mut() {
            if let Some(client) = entry.client.take() {
                let _ = client.shutdown().await;
            }
            entry.connected = false;
        }
    }
}

pub fn parse_mcp_tool_arguments(arguments: &str) -> Result<serde_json::Value, anyhow::Error> {
    serde_json::from_str(arguments)
        .map_err(|e| anyhow::anyhow!("invalid MCP tool arguments JSON: {e}"))
}

fn parse_server_tool(server_tool: &str) -> Option<(&str, &str)> {
    let (server, tool) = server_tool.split_once('.')?;
    if server.is_empty() || tool.is_empty() || tool.contains('.') {
        return None;
    }
    Some((server, tool))
}

fn filter_tools(tools: Vec<McpTool>, options: &McpServerOptions) -> Vec<McpTool> {
    tools
        .into_iter()
        .filter(|tool| options.allows_tool(&tool.name))
        .collect()
}

impl Default for McpRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_empty() {
        let reg = McpRegistry::new();
        assert!(!reg.has_connected());
        assert!(reg.all_tools().is_empty());
    }

    #[test]
    fn test_registry_status_empty() {
        let reg = McpRegistry::new();
        assert!(reg.status().contains("MCP Servers"));
    }

    #[test]
    fn filter_tools_respects_include_and_exclude() {
        let options = McpServerOptions {
            include_tools: vec!["read".into(), "write".into()],
            exclude_tools: vec!["write".into()],
            trust: false,
            timeout_ms: None,
        };
        let tools = vec![tool("read"), tool("write"), tool("search")];

        let names: Vec<String> = filter_tools(tools, &options)
            .into_iter()
            .map(|tool| tool.name)
            .collect();

        assert_eq!(names, vec!["read"]);
    }

    #[test]
    fn parse_mcp_tool_arguments_rejects_invalid_json() {
        let err = parse_mcp_tool_arguments("{not json")
            .expect_err("invalid JSON should be surfaced to the caller");
        assert!(err.to_string().contains("invalid MCP tool arguments JSON"));
    }

    #[test]
    fn register_config_copies_per_server_options() {
        let server_config = crate::storage::config::McpServerEntryConfig {
            transport: crate::mcp::client::McpTransport::Stdio,
            command: Some("cmd".into()),
            args: vec!["--serve".into()],
            env: None,
            url: None,
            headers: None,
            include_tools: vec!["read".into()],
            exclude_tools: vec!["write".into()],
            trust: true,
            timeout_ms: 1_234,
        };
        let mut reg = McpRegistry::new();

        reg.register_config("fs".into(), &server_config);

        let entry = reg.servers.get("fs").expect("server should be registered");
        assert_eq!(entry.options.include_tools, vec!["read"]);
        assert_eq!(entry.options.exclude_tools, vec!["write"]);
        assert!(entry.options.trust);
        assert_eq!(entry.options.timeout_ms(), 1_234);
        assert_eq!(
            entry.config.transport,
            crate::mcp::client::McpTransport::Stdio
        );
        assert_eq!(entry.config.command.as_deref(), Some("cmd"));
    }

    #[test]
    fn tool_metadata_reports_source_filters_and_advertised_state() {
        let server_config = crate::storage::config::McpServerEntryConfig {
            transport: crate::mcp::client::McpTransport::Stdio,
            command: Some("cmd".into()),
            args: Vec::new(),
            env: None,
            url: None,
            headers: None,
            include_tools: vec!["read".into()],
            exclude_tools: vec!["write".into()],
            trust: true,
            timeout_ms: 1_234,
        };
        let mut reg = McpRegistry::new();
        reg.register_config("fs".into(), &server_config);
        let entry = reg.servers.get_mut("fs").expect("server");
        entry.connected = true;
        entry.tools = vec![tool("read")];

        assert!(reg.is_mcp_tool_name("fs.read"));
        let metadata = reg
            .tool_approval_metadata("fs.read")
            .expect("metadata for advertised tool");
        assert_eq!(metadata.identity.source, "mcp:fs.read");
        assert!(metadata.trust);
        assert!(metadata.advertised);
        assert!(metadata.allowed_by_filters);

        let blocked = reg
            .tool_approval_metadata("fs.write")
            .expect("metadata for filtered tool");
        assert!(!blocked.advertised);
        assert!(!blocked.allowed_by_filters);
    }

    #[test]
    fn all_tools_are_ordered_by_server_name() {
        let server_config = crate::storage::config::McpServerEntryConfig {
            transport: crate::mcp::client::McpTransport::Stdio,
            command: Some("cmd".into()),
            args: Vec::new(),
            env: None,
            url: None,
            headers: None,
            include_tools: Vec::new(),
            exclude_tools: Vec::new(),
            trust: true,
            timeout_ms: 1_234,
        };
        let mut reg = McpRegistry::new();
        // Register out of alphabetical order; all_tools must still come back
        // sorted so the MCP tool prefix stays stable for the context cache.
        for name in ["zeta", "alpha", "mu"] {
            reg.register_config(name.to_string(), &server_config);
            let entry = reg.servers.get_mut(name).expect("server");
            entry.connected = true;
            entry.tools = vec![tool(name)];
        }

        let servers: Vec<&str> = reg.all_tools().into_iter().map(|(s, _)| s).collect();
        assert_eq!(servers, vec!["alpha", "mu", "zeta"]);
    }

    fn tool(name: &str) -> McpTool {
        McpTool {
            name: name.into(),
            description: None,
            input_schema: serde_json::json!({}),
        }
    }
}
