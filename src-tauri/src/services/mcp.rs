//! MCP (Model Context Protocol) client — Phase 9.0.3.
//!
//! Manages MCP server configurations and stdio connections through RMCP,
//! discovers remote tools, invokes them, and exposes helpers that wire
//! discovered tools into the Phase 9.0.1 native tool-calling surface
//! (`mcp__<server>__<tool>` naming) so agent runs can see and call them.

use crate::services::workspace;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation, JsonObject,
};
use rmcp::service::{RoleClient, ServiceExt};
use rmcp::transport::TokioChildProcess;

/// One configured MCP server (stdio transport: a command to spawn).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct McpServersConfig {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

impl McpServersConfig {
    pub fn load() -> Self {
        Self::load_from_dir(&workspace::config_dir())
    }

    pub fn load_from_dir(dir: &std::path::Path) -> Self {
        match std::fs::read_to_string(dir.join("mcp.json")) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to_dir(&workspace::config_dir())
    }

    pub fn save_to_dir(&self, dir: &std::path::Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("Create config dir: {}", e))?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Serialize MCP config: {}", e))?;
        std::fs::write(dir.join("mcp.json"), json)
            .map_err(|e| format!("Write MCP config: {}", e))
    }

    /// Validate names/commands so a broken config cannot spawn garbage.
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for server in &self.servers {
            let name = server.name.trim();
            if name.is_empty() {
                return Err("MCP server name must not be empty".to_string());
            }
            if !seen.insert(name.to_lowercase()) {
                return Err(format!("Duplicate MCP server name: {}", name));
            }
            if server.command.trim().is_empty() {
                return Err(format!(
                    "MCP server '{}' must define a command",
                    server.name
                ));
            }
        }
        Ok(())
    }
}

/// A discovered MCP tool, normalized for the agent tool surface.
#[derive(Clone, Debug, Serialize)]
pub struct McpToolInfo {
    pub name: String,
    #[serde(rename = "description")]
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// The outcome of an MCP tool invocation.
#[derive(Clone, Debug, Serialize)]
pub struct McpToolCallOutcome {
    pub server: String,
    pub tool: String,
    #[serde(rename = "isError")]
    pub is_error: bool,
    pub text: String,
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<serde_json::Value>,
}

/// A live stdio MCP connection backed by RMCP's client service.
pub struct McpConnection {
    server_name: String,
    running: rmcp::service::RunningService<RoleClient, ClientConfig>,
}

impl McpConnection {
    pub async fn connect(config: &McpServerConfig) -> Result<Self, String> {
        let mut command = tokio::process::Command::new(&config.command);
        command.args(&config.args);
        command.envs(&config.env);
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            // Hide the console window that spawned MCP servers would open.
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        let transport = TokioChildProcess::new(command)
            .map_err(|e| format!("Spawn MCP server '{}': {}", config.name, e))?;

        let client_config = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("agent-ide", env!("CARGO_PKG_VERSION")),
        );

        let running = client_config
            .serve(transport)
            .await
            .map_err(|e| format!("MCP initialize failed for '{}': {}", config.name, e))?;

        Ok(Self {
            server_name: config.name.clone(),
            running,
        })
    }

    pub async fn disconnect(self: Arc<Self>) {
        // The token cancels the service loop without needing ownership of
        // the RunningService itself (we hold it behind an Arc).
        self.running.cancellation_token().cancel();
    }

    /// Discover the tools this server exposes.
    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>, String> {
        let tools = self
            .running
            .list_all_tools()
            .await
            .map_err(|e| format!("MCP list_tools failed for '{}': {}", self.server_name, e))?;
        Ok(tools
            .into_iter()
            .map(|tool| McpToolInfo {
                name: tool.name.to_string(),
                description: tool.description.map(|d| d.to_string()),
                input_schema: serde_json::Value::Object(
                    (*tool.input_schema).clone(),
                ),
            })
            .collect())
    }

    /// Invoke one tool on this server.
    pub async fn call_tool(
        &self,
        tool: &str,
        arguments: Option<JsonObject>,
    ) -> Result<McpToolCallOutcome, String> {
        let mut params = CallToolRequestParams::new(tool.to_string());
        if let Some(arguments) = arguments {
            params = params.with_arguments(arguments);
        }
        let result = self
            .running
            .call_tool(params)
            .await
            .map_err(|e| format!("MCP call_tool '{}' failed: {}", tool, e))?;

        let text = result
            .content
            .iter()
            .filter_map(|block| match block {
                rmcp::model::ContentBlock::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(McpToolCallOutcome {
            server: self.server_name.clone(),
            tool: tool.to_string(),
            is_error: result.is_error.unwrap_or(false),
            text,
            structured_content: result.structured_content,
        })
    }
}

/// Tauri-managed registry of live MCP connections.
#[derive(Default)]
pub struct McpState {
    connections: std::sync::Mutex<HashMap<String, Arc<McpConnection>>>,
}

impl McpState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn connected_servers(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    pub fn is_connected(&self, server: &str) -> bool {
        self.lock().contains_key(server)
    }

    /// Spawn and initialize a server connection, replacing any existing one.
    pub async fn connect(&self, config: &McpServerConfig) -> Result<(), String> {
        let connection = Arc::new(McpConnection::connect(config).await?);
        let previous = self
            .lock()
            .insert(config.name.clone(), Arc::clone(&connection));
        if let Some(previous) = previous {
            previous.disconnect().await;
        }
        Ok(())
    }

    pub async fn disconnect(&self, server: &str) -> Result<(), String> {
        let removed = self.lock().remove(server);
        match removed {
            Some(connection) => {
                connection.disconnect().await;
                Ok(())
            }
            None => Err(format!("MCP server '{}' is not connected", server)),
        }
    }

    /// Discover tools across every enabled, connected server.
    pub async fn snapshot_tools(&self) -> Vec<(String, McpToolInfo)> {
        let connections: Vec<(String, Arc<McpConnection>)> = self
            .lock()
            .iter()
            .map(|(name, connection)| (name.clone(), Arc::clone(connection)))
            .collect();

        let mut snapshot = Vec::new();
        for (server, connection) in connections {
            match connection.list_tools().await {
                Ok(tools) => tools.into_iter().for_each(|tool| {
                    snapshot.push((server.clone(), tool));
                }),
                Err(error) => {
                    eprintln!("MCP tool discovery failed for '{}': {}", server, error);
                }
            }
        }
        snapshot
    }

    /// Snapshot discovered tools as OpenAI function-shaped schemas with
    /// `mcp__<server>__<tool>` surface names, ready for `LlmClient::with_extra_tools`.
    pub async fn tool_function_schemas(&self) -> Vec<serde_json::Value> {
        self.snapshot_tools()
            .await
            .into_iter()
            .map(|(server, tool)| {
                let surface_name = mcp_tool_surface_name(&server, &tool.name);
                let description = tool.description.clone().unwrap_or_else(|| {
                    format!("MCP tool {} on server {}", tool.name, server)
                });
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": surface_name,
                        "description": description,
                        "parameters": tool.input_schema,
                    }
                })
            })
            .collect()
    }

    /// Invoke a tool on a connected server.
    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: Option<JsonObject>,
    ) -> Result<McpToolCallOutcome, String> {
        let connection = self
            .lock()
            .get(server)
            .cloned()
            .ok_or_else(|| format!("MCP server '{}' is not connected", server))?;
        connection.call_tool(tool, arguments).await
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<McpConnection>>> {
        self.connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The name an MCP tool carries on the agent tool surface:
/// `mcp__<server>__<tool>`.
pub fn mcp_tool_surface_name(server: &str, tool: &str) -> String {
    format!("mcp__{}__{}", server, tool)
}

/// Split an agent-surface tool name back into `(server, tool)`.
pub fn parse_mcp_tool_surface_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server.to_string(), tool.to_string()))
}

/// Merge discovered MCP tools into the native tools schema so an
/// OpenAI-compatible request exposes them alongside the built-in tools.
/// Tools are renamed to the `mcp__<server>__<tool>` surface name.
pub fn augment_native_tools_with_mcp(
    native_schema: serde_json::Value,
    mcp_tools: &[(String, McpToolInfo)],
) -> serde_json::Value {
    let mut schema = native_schema;
    let Some(array) = schema.as_array_mut() else {
        return schema;
    };
    for (server, tool) in mcp_tools {
        array.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": mcp_tool_surface_name(server, &tool.name),
                "description": tool.description.clone().unwrap_or_else(|| {
                    format!("MCP tool {} on server {}", tool.name, server)
                }),
                "parameters": tool.input_schema.clone(),
            }
        }));
    }
    schema
}

/// Serialize an MCP outcome into the fenced block appended to a model
/// response so downstream stages and the chat view can see the result.
pub fn format_mcp_tool_result_block(outcome: &McpToolCallOutcome) -> String {
    let payload = serde_json::json!({
        "server": outcome.server,
        "tool": outcome.tool,
        "isError": outcome.is_error,
        "text": outcome.text,
        "structuredContent": outcome.structured_content,
    });
    format!(
        "\n```mcp-tool-result\n{}\n```\n",
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str, description: Option<&str>) -> McpToolInfo {
        McpToolInfo {
            name: name.to_string(),
            description: description.map(|d| d.to_string()),
            input_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn temp_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("agent-ide-mcp-test-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn config_round_trips_through_disk_and_validates() {
        let dir = temp_dir();
        let config = McpServersConfig {
            servers: vec![McpServerConfig {
                name: "filesystem".to_string(),
                command: "npx".to_string(),
                args: vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string()],
                env: HashMap::new(),
                enabled: true,
            }],
        };
        config.save_to_dir(&dir).unwrap();
        let loaded = McpServersConfig::load_from_dir(&dir);
        assert_eq!(loaded.servers.len(), 1);
        assert_eq!(loaded.servers[0].name, "filesystem");
        assert!(loaded.validate().is_ok());

        let mut duplicate = McpServersConfig {
            servers: vec![
                McpServerConfig {
                    name: "a".to_string(),
                    command: "x".to_string(),
                    args: vec![],
                    env: HashMap::new(),
                    enabled: true,
                },
                McpServerConfig {
                    name: "A".to_string(),
                    command: "y".to_string(),
                    args: vec![],
                    env: HashMap::new(),
                    enabled: true,
                },
            ],
        };
        assert!(duplicate.validate().is_err());
        duplicate.servers[0].command = String::new();
        assert!(duplicate.validate().is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_config_falls_back_to_default() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mcp.json"), "{not json").unwrap();
        assert!(McpServersConfig::load_from_dir(&dir).servers.is_empty());
        assert!(McpServersConfig::load_from_dir(&dir).validate().is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tool_surface_names_round_trip() {
        assert_eq!(
            mcp_tool_surface_name("filesystem", "read_file"),
            "mcp__filesystem__read_file"
        );
        assert_eq!(
            parse_mcp_tool_surface_name("mcp__filesystem__read_file"),
            Some(("filesystem".to_string(), "read_file".to_string()))
        );
        assert_eq!(parse_mcp_tool_surface_name("emit_agent_changes"), None);
        assert_eq!(parse_mcp_tool_surface_name("mcp__server__"), None);
        assert_eq!(parse_mcp_tool_surface_name("mcp____tool"), None);
    }

    #[test]
    fn augment_appends_mcp_tools_as_function_schemas() {
        let native = serde_json::json!([
            {"type": "function", "function": {"name": "emit_agent_changes"}}
        ]);
        let augmented = augment_native_tools_with_mcp(
            native,
            &[
                ("fs".to_string(), tool("read_file", Some("Read a file"))),
                ("fs".to_string(), tool("no_desc", None)),
            ],
        );
        let array = augmented.as_array().unwrap();
        assert_eq!(array.len(), 3);
        assert_eq!(array[1]["function"]["name"], "mcp__fs__read_file");
        assert_eq!(array[1]["function"]["description"], "Read a file");
        assert_eq!(
            array[2]["function"]["description"],
            "MCP tool no_desc on server fs"
        );
        assert_eq!(array[2]["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn augment_keeps_non_array_payload_untouched() {
        let payload = serde_json::json!({"unexpected": true});
        assert_eq!(
            augment_native_tools_with_mcp(payload.clone(), &[("s".to_string(), tool("t", None))]),
            payload
        );
    }

    #[test]
    fn outcome_block_is_fenced_json_with_identity() {
        let outcome = McpToolCallOutcome {
            server: "fs".to_string(),
            tool: "read_file".to_string(),
            is_error: false,
            text: "file contents".to_string(),
            structured_content: Some(serde_json::json!({"bytes": 12})),
        };
        let block = format_mcp_tool_result_block(&outcome);
        assert!(block.contains("```mcp-tool-result"));
        assert!(block.contains("\"server\": \"fs\""));
        assert!(block.contains("file contents"));
        assert!(!outcome.is_error);
    }

    #[tokio::test]
    async fn state_reports_unconnected_servers() {
        let state = McpState::new();
        assert!(state.connected_servers().is_empty());
        assert!(state.snapshot_tools().await.is_empty());
        assert!(state.call_tool("missing", "tool", None).await.is_err());
        assert!(state.disconnect("missing").await.is_err());
    }
}
