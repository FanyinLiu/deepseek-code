//! MCP server registry — manages multiple MCP connections and aggregates their tools.
use std::collections::HashMap;

use super::client::{McpClient, McpServerConfig, DEFAULT_MCP_TIMEOUT_MS};
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
}

impl McpRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
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
                command: server_config.command.clone(),
                args: server_config.args.clone(),
                env: server_config.env.clone(),
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
        for (name, entry) in &self.servers {
            if entry.connected {
                for tool in &entry.tools {
                    result.push((name.as_str(), tool));
                }
            }
        }
        result
    }

    /// Call a tool by its server-prefixed name (`server_name.tool_name`).
    pub async fn call_tool(
        &mut self,
        server_tool: &str,
        arguments: serde_json::Value,
    ) -> Result<String, anyhow::Error> {
        let parts: Vec<&str> = server_tool.splitn(2, '.').collect();
        if parts.len() != 2 {
            anyhow::bail!("tool name must be 'server_name.tool_name', got: {server_tool}");
        }
        let (server_name, tool_name) = (parts[0], parts[1]);

        let entry = self
            .servers
            .get_mut(server_name)
            .ok_or_else(|| anyhow::anyhow!("unknown MCP server: {server_name}"))?;

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
            command: "cmd".into(),
            args: vec!["--serve".into()],
            env: None,
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
    }

    fn tool(name: &str) -> McpTool {
        McpTool {
            name: name.into(),
            description: None,
            input_schema: serde_json::json!({}),
        }
    }
}
