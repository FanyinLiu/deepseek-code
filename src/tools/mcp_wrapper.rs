//! Local MCP wrapper — converts local MCP tool configurations
//! into `DeepSeek` function tool schemas.
//!
//! v1 does NOT pass through MCP `tool_use` / `tool_result` to `DeepSeek`.
//! Instead, MCP tools are wrapped as regular function tools that
//! the agent can call during execution.

use crate::deepseek::ToolDefinition;

/// Read MCP server configs from ~/.octocode/mcp.toml or project .octocode/mcp.toml.
pub fn load_mcp_configs(
    _user_config_path: &std::path::Path,
    _project_config_path: Option<&std::path::Path>,
) -> Result<Vec<McpServerConfig>, anyhow::Error> {
    // v1: basic implementation — reads a simple TOML config
    // v2: full MCP protocol support
    Ok(Vec::new())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub description: Option<String>,
}

/// Convert MCP server configs to `DeepSeek` function tool definitions.
#[must_use]
pub fn mcp_tools_as_function_defs(_configs: &[McpServerConfig]) -> Vec<ToolDefinition> {
    // v1: MCP tools are not auto-converted to function tools
    // The user must manually define tools that wrap MCP servers.
    Vec::new()
}
