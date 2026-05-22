//! MCP (Model Context Protocol) support.
//!
//! Provides a client and registry for connecting to MCP servers over stdio.
//! MCP tools are dynamically discovered and seamlessly integrated into the agent's tool loop.
pub mod approval_bridge;
pub mod client;
pub mod protocol;
pub mod registry;
pub mod server;

pub use client::{McpClient, McpServerConfig, McpTransport};
pub use registry::McpRegistry;
pub use server::serve_stdio;
