//! Tauri commands for the MCP client (Phase 9.0.3).

use crate::services::mcp::{
    McpServerConfig, McpServersConfig, McpState, McpToolCallOutcome, McpToolInfo,
};
use tauri::State;

#[tauri::command]
pub async fn mcp_get_servers() -> Result<McpServersConfig, String> {
    let config = McpServersConfig::load();
    config.validate()?;
    Ok(config)
}

#[tauri::command]
pub async fn mcp_set_servers(config: McpServersConfig) -> Result<(), String> {
    config.validate()?;
    config.save()
}

#[tauri::command]
pub async fn mcp_connect(name: String, mcp: State<'_, McpState>) -> Result<(), String> {
    let config = McpServersConfig::load();
    config.validate()?;
    let server = config
        .servers
        .into_iter()
        .find(|server| server.name == name && server.enabled)
        .ok_or_else(|| format!("MCP server '{}' is not configured or disabled", name))?;
    mcp.connect(&server).await
}

#[tauri::command]
pub async fn mcp_disconnect(name: String, mcp: State<'_, McpState>) -> Result<(), String> {
    mcp.disconnect(&name).await
}

#[tauri::command]
pub async fn mcp_status(mcp: State<'_, McpState>) -> Result<McpStatusResponse, String> {
    Ok(McpStatusResponse {
        servers: McpServersConfig::load().servers,
        connected: mcp.connected_servers(),
    })
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct McpStatusResponse {
    pub servers: Vec<McpServerConfig>,
    pub connected: Vec<String>,
}

#[tauri::command]
pub async fn mcp_list_tools(mcp: State<'_, McpState>) -> Result<Vec<(String, McpToolInfo)>, String> {
    Ok(mcp.snapshot_tools().await)
}

#[tauri::command]
pub async fn mcp_call_tool(
    server: String,
    tool: String,
    arguments: Option<serde_json::Value>,
    mcp: State<'_, McpState>,
) -> Result<McpToolCallOutcome, String> {
    let arguments = match arguments {
        Some(serde_json::Value::Object(map)) => Some(map),
        Some(serde_json::Value::Null) | None => None,
        Some(other) => {
            return Err(format!(
                "MCP tool arguments must be a JSON object, got: {}",
                other
            ))
        }
    };
    mcp.call_tool(&server, &tool, arguments).await
}
