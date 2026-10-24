//! End-to-end MCP integration test (Phase 9.0.3): connects the RMCP stdio
//! client to a real child-process MCP server (`tests/mcp_mock_server.mjs`,
//! a newline-delimited JSON-RPC implementation) and exercises the full
//! lifecycle: initialize handshake -> tool discovery -> tool invocation,
//! including the error path.

use agent_ide_lib::services::mcp::{
    augment_native_tools_with_mcp, mcp_tool_surface_name, parse_mcp_tool_surface_name,
    McpServerConfig, McpServersConfig, McpState,
};
use std::collections::HashMap;

fn mock_server_config() -> McpServerConfig {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/mcp_mock_server.mjs");
    McpServerConfig {
        name: "mock".to_string(),
        command: "node".to_string(),
        args: vec![script.to_string_lossy().to_string()],
        env: HashMap::new(),
        enabled: true,
    }
}

#[tokio::test]
async fn mcp_client_connects_discovers_and_calls_tools() {
    let state = McpState::new();
    state.connect(&mock_server_config()).await.expect("connect");

    assert!(state.is_connected("mock"));
    assert_eq!(state.connected_servers(), vec!["mock".to_string()]);

    // Discovery: both mock tools are visible with schemas.
    let tools = state.snapshot_tools().await;
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].0, "mock");
    assert_eq!(tools[0].1.name, "echo");
    assert_eq!(
        tools[0].1.input_schema.get("type").and_then(|v| v.as_str()),
        Some("object")
    );

    // Surface naming round-trips into the agent tool surface.
    let surface = mcp_tool_surface_name("mock", "echo");
    assert_eq!(parse_mcp_tool_surface_name(&surface).unwrap().1, "echo");

    // Invocation success path.
    let arguments = serde_json::json!({ "text": "hello mcp" })
        .as_object()
        .cloned();
    let outcome = state
        .call_tool("mock", "echo", arguments)
        .await
        .expect("call echo");
    assert!(!outcome.is_error);
    assert_eq!(outcome.text, "echo: hello mcp");

    // Invocation error path surfaces the tool-level error.
    let outcome = state
        .call_tool("mock", "fail", None)
        .await
        .expect("call fail");
    assert!(outcome.is_error);
    assert_eq!(outcome.text, "mock failure");

    state.disconnect("mock").await.expect("disconnect");
    assert!(!state.is_connected("mock"));
}

#[tokio::test]
async fn connecting_twice_replaces_the_previous_connection() {
    let state = McpState::new();
    state.connect(&mock_server_config()).await.expect("first");
    state.connect(&mock_server_config()).await.expect("second");
    assert!(state.is_connected("mock"));
    // The replaced connection is still functional through the new handle.
    let tools = state.snapshot_tools().await;
    assert_eq!(tools.len(), 2);
    state.disconnect("mock").await.expect("disconnect");
}

#[test]
fn discovered_tools_augment_the_native_schema() {
    let tools = vec![(
        "mock".to_string(),
        agent_ide_lib::services::mcp::McpToolInfo {
            name: "echo".to_string(),
            description: Some("Echo the provided text back to the caller".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }),
        },
    )];
    let augmented = augment_native_tools_with_mcp(serde_json::json!([]), &tools);
    let array = augmented.as_array().unwrap();
    assert_eq!(array.len(), 1);
    assert_eq!(array[0]["function"]["name"], "mcp__mock__echo");
    assert_eq!(array[0]["function"]["parameters"]["type"], "object");
}

#[test]
fn config_persists_and_validates() {
    let dir = std::env::temp_dir().join(format!("agent-ide-mcp-it-{}", uuid::Uuid::new_v4()));
    let config = McpServersConfig {
        servers: vec![mock_server_config()],
    };
    config.save_to_dir(&dir).expect("save");
    let loaded = McpServersConfig::load_from_dir(&dir);
    assert_eq!(loaded.servers.len(), 1);
    assert!(loaded.validate().is_ok());
    std::fs::remove_dir_all(&dir).ok();
}
