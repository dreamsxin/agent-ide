//! Agent 在运行中主动读取工作区的内置工具。
//!
//! 在这之前，Agent 只能看到一次运行开始时打包好的上下文（活动文件、选区、
//! 目录树摘要、git diff）。要改一个用户没有预先选中的文件，模型只能凭猜测
//! 写出 `ORIGINAL` 段 —— 这正是 "Could not find original content" 类应用失败
//! 的来源。现代 agent 的做法是把"读什么"交给模型自己决定，用工具调用去取。
//!
//! 这些工具刻意不走 MCP：
//! - MCP 工具的参数完全不受约束（见 SECURITY.md），而这里每个路径都过
//!   `resolve_existing`，并且拒绝凭据文件，与上下文出网过滤保持一致。
//! - 不需要用户配置任何外部进程就能用。

use crate::agent::executor::ToolInvoker;
use crate::services::llm_client::ToolDefinition;
use crate::services::workspace;
use async_trait::async_trait;
use std::path::Path;

/// 内置工作区工具的名字前缀，与 MCP 的 `mcp__` 前缀互不重叠，
/// 这样 `handles` 的判定是确定的。
pub const WORKSPACE_TOOL_PREFIX: &str = "workspace_";

pub const READ_FILE: &str = "workspace_read_file";
pub const SEARCH_TEXT: &str = "workspace_search_text";
pub const LIST_FILES: &str = "workspace_list_files";

/// 单个文件最多回传的字节数，避免一次调用就吃掉整个上下文预算
const MAX_READ_BYTES: usize = 64_000;
/// 搜索最多回传的匹配行数
const MAX_SEARCH_RESULTS: usize = 60;
/// 列目录最多回传的条目数
const MAX_LIST_ENTRIES: usize = 200;
/// 遍历时跳过的目录：构建产物和依赖树，不是源码
const SKIPPED_DIRS: [&str; 6] = [
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".agent-ide",
];

pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: READ_FILE.to_string(),
            description:
                "Read a UTF-8 text file from the workspace. Use this before proposing an edit so \
                 the ORIGINAL section matches the file exactly. Paths are relative to the \
                 workspace root."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path, e.g. src/app.ts"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: SEARCH_TEXT.to_string(),
            description:
                "Search workspace file contents for a literal substring and return matching \
                 file paths with line numbers. Use this to locate the code you need to change."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Literal substring to look for (not a regular expression)"
                    },
                    "extension": {
                        "type": "string",
                        "description": "Optional file extension filter without the dot, e.g. ts"
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: LIST_FILES.to_string(),
            description: "List files and directories under a workspace directory, one level deep."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative directory, defaults to the workspace root"
                    }
                }
            }),
        },
    ]
}

pub struct WorkspaceToolInvoker;

#[async_trait]
impl ToolInvoker for WorkspaceToolInvoker {
    fn handles(&self, tool_name: &str) -> bool {
        matches!(tool_name, READ_FILE | SEARCH_TEXT | LIST_FILES)
    }

    async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String> {
        let args: serde_json::Value = serde_json::from_str(arguments.trim())
            .map_err(|error| format!("Tool arguments are not valid JSON: {}", error))?;
        match tool_name {
            READ_FILE => read_file_tool(string_arg(&args, "path").ok_or("Missing 'path'")?),
            SEARCH_TEXT => search_text_tool(
                string_arg(&args, "query").ok_or("Missing 'query'")?,
                string_arg(&args, "extension"),
            ),
            LIST_FILES => list_files_tool(string_arg(&args, "path").unwrap_or(".")),
            other => Err(format!("Unknown workspace tool: {}", other)),
        }
    }
}

fn string_arg<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// 凭据文件对 Agent 一律不可读。
///
/// 上下文构建那边已经把 `.env` 的内容挡掉了，如果这里放行，等于给模型开了
/// 一条绕过出网过滤的后门 —— 它只要主动调一次读取工具就能拿到同样的内容。
fn reject_credential_path(path: &str) -> Result<(), String> {
    if workspace::is_credential_path(path) {
        return Err(format!(
            "Reading {} is not allowed: it looks like a credential file",
            path
        ));
    }
    Ok(())
}

fn read_file_tool(path: &str) -> Result<String, String> {
    reject_credential_path(path)?;
    let resolved = workspace::resolve_existing(path)?;
    let content =
        std::fs::read_to_string(&resolved).map_err(|error| format!("Read {}: {}", path, error))?;
    if content.len() <= MAX_READ_BYTES {
        return Ok(content);
    }
    let head: String = content.chars().take(MAX_READ_BYTES).collect();
    Ok(format!(
        "{}\n... [truncated at {} bytes; read a narrower range or search instead]",
        head, MAX_READ_BYTES
    ))
}

fn list_files_tool(path: &str) -> Result<String, String> {
    let resolved = workspace::resolve_existing(path)?;
    let mut entries: Vec<String> = Vec::new();
    let read_dir =
        std::fs::read_dir(&resolved).map_err(|error| format!("List {}: {}", path, error))?;
    for entry in read_dir.flatten() {
        if entries.len() >= MAX_LIST_ENTRIES {
            entries.push("... [truncated]".to_string());
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if SKIPPED_DIRS.contains(&name.as_str()) {
            continue;
        }
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        entries.push(if is_dir { format!("{}/", name) } else { name });
    }
    entries.sort();
    if entries.is_empty() {
        return Ok(format!("{} is empty", path));
    }
    Ok(entries.join("\n"))
}

fn search_text_tool(query: &str, extension: Option<&str>) -> Result<String, String> {
    let root = workspace::workspace_root()?;
    let mut matches: Vec<String> = Vec::new();
    walk_and_match(&root, &root, query, extension, &mut matches);
    if matches.is_empty() {
        return Ok(format!("No matches for {:?}", query));
    }
    let truncated = matches.len() > MAX_SEARCH_RESULTS;
    matches.truncate(MAX_SEARCH_RESULTS);
    if truncated {
        matches.push("... [more matches omitted; narrow the query]".to_string());
    }
    Ok(matches.join("\n"))
}

fn walk_and_match(
    root: &Path,
    dir: &Path,
    query: &str,
    extension: Option<&str>,
    matches: &mut Vec<String>,
) {
    if matches.len() > MAX_SEARCH_RESULTS {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        if matches.len() > MAX_SEARCH_RESULTS {
            return;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if is_dir {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                walk_and_match(root, &path, query, extension, matches);
            }
            continue;
        }
        // 凭据文件不出现在搜索结果里，和读取工具、上下文过滤保持一致
        if workspace::is_credential_path(&name) {
            continue;
        }
        if let Some(extension) = extension {
            if path.extension().and_then(|value| value.to_str()) != Some(extension) {
                continue;
            }
        }
        // 二进制文件读不成 UTF-8，直接跳过而不是报错
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let relative = relative.to_string_lossy().replace('\\', "/");
        for (index, line) in content.lines().enumerate() {
            if line.contains(query) {
                matches.push(format!("{}:{}: {}", relative, index + 1, line.trim()));
                if matches.len() > MAX_SEARCH_RESULTS {
                    return;
                }
            }
        }
    }
}

/// 把多个执行器合成一个。
///
/// `select_external_calls` 只接受一个 `Option<&dyn ToolInvoker>`，而一次运行里
/// 内置工作区工具和 MCP 工具都可能启用，所以这里按顺序找第一个认领该名字的。
pub struct CompositeToolInvoker {
    invokers: Vec<std::sync::Arc<dyn ToolInvoker>>,
}

impl CompositeToolInvoker {
    pub fn new(invokers: Vec<std::sync::Arc<dyn ToolInvoker>>) -> Self {
        Self { invokers }
    }
}

#[async_trait]
impl ToolInvoker for CompositeToolInvoker {
    fn handles(&self, tool_name: &str) -> bool {
        self.invokers
            .iter()
            .any(|invoker| invoker.handles(tool_name))
    }

    async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String> {
        for invoker in &self.invokers {
            if invoker.handles(tool_name) {
                return invoker.invoke(tool_name, arguments).await;
            }
        }
        Err(format!("No invoker handles tool {}", tool_name))
    }
}

/// 把内置工作区工具接到一次运行上，并与已有的（MCP）执行器合并。
///
/// 内置工具无条件启用：它们只读、受工作区边界约束、且拒绝凭据文件，
/// 不像 MCP 那样需要用户先信任一个外部进程。
pub fn attach_workspace_tools(
    llm: crate::services::llm_client::LlmClient,
    existing: Option<std::sync::Arc<dyn ToolInvoker>>,
) -> (
    crate::services::llm_client::LlmClient,
    Option<std::sync::Arc<dyn ToolInvoker>>,
) {
    let mut definitions = llm.extra_tools().to_vec();
    definitions.extend(tool_definitions());

    let mut invokers: Vec<std::sync::Arc<dyn ToolInvoker>> =
        vec![std::sync::Arc::new(WorkspaceToolInvoker)];
    if let Some(existing) = existing {
        invokers.push(existing);
    }

    (
        llm.with_extra_tools(definitions),
        Some(std::sync::Arc::new(CompositeToolInvoker::new(invokers))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct TestEnv {
        root: std::path::PathBuf,
        config_dir: std::path::PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let base =
                std::env::temp_dir().join(format!("agent-ide-tools-test-{}", Uuid::new_v4()));
            let root = base.join("workspace");
            let config_dir = base.join("config");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&config_dir).unwrap();
            let root = workspace::shell_compatible_path(root.canonicalize().unwrap());
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &config_dir);
            workspace::save_workspace_path(root.to_string_lossy().as_ref()).unwrap();
            Self { root, config_dir }
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            std::env::remove_var("AGENT_IDE_CONFIG_DIR");
            let _ = std::fs::remove_dir_all(self.root.parent().unwrap_or(&self.root));
            let _ = std::fs::remove_dir_all(&self.config_dir);
        }
    }

    #[test]
    fn read_file_returns_workspace_content_and_refuses_escapes() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/app.ts", "const value = 1;\n");

        assert_eq!(read_file_tool("src/app.ts").unwrap(), "const value = 1;\n");

        let err = read_file_tool("../outside.txt").unwrap_err();
        assert!(
            err.contains("outside workspace") || err.contains("does not exist"),
            "{}",
            err
        );
    }

    /// 上下文构建已经挡掉了 `.env` 的内容；如果读取工具放行，模型只要主动调
    /// 一次就能拿到同样的东西，出网过滤等于白做。
    #[test]
    fn credential_files_are_unreadable_and_unsearchable() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write(".env", "STRIPE_SECRET_KEY=sk_live_deadbeef\n");
        env.write("src/app.ts", "const token = readEnv();\n");

        let err = read_file_tool(".env").unwrap_err();
        assert!(err.contains("credential"), "{}", err);

        // 搜不到：`.env` 不参与遍历。注意"无匹配"的回执里会回显查询串本身，
        // 所以这里断言的是没有命中该文件，而不是回执里不含这段文本。
        let results = search_text_tool("STRIPE_SECRET_KEY", None).unwrap();
        assert!(!results.contains(".env"), "{}", results);
        assert!(results.starts_with("No matches"), "{}", results);

        // 普通文件照常可搜
        let results = search_text_tool("readEnv", None).unwrap();
        assert!(results.contains("src/app.ts:1"), "{}", results);
    }

    #[test]
    fn search_filters_by_extension_and_skips_dependency_dirs() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/app.ts", "findMe();\n");
        env.write("src/app.py", "findMe()\n");
        env.write("node_modules/pkg/index.ts", "findMe();\n");

        let all = search_text_tool("findMe", None).unwrap();
        assert!(all.contains("src/app.ts"));
        assert!(all.contains("src/app.py"));
        // 依赖树里的命中是噪音，会把上下文预算吃光
        assert!(!all.contains("node_modules"), "{}", all);

        let only_ts = search_text_tool("findMe", Some("ts")).unwrap();
        assert!(only_ts.contains("src/app.ts"));
        assert!(!only_ts.contains("app.py"), "{}", only_ts);
    }

    #[test]
    fn list_files_hides_dependency_dirs() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/app.ts", "");
        env.write("node_modules/pkg/index.js", "");
        env.write("README.md", "");

        let listing = list_files_tool(".").unwrap();

        assert!(listing.contains("src/"));
        assert!(listing.contains("README.md"));
        assert!(!listing.contains("node_modules"), "{}", listing);
    }

    #[test]
    fn tool_names_do_not_collide_with_mcp_routing() {
        let invoker = WorkspaceToolInvoker;
        for definition in tool_definitions() {
            assert!(definition.name.starts_with(WORKSPACE_TOOL_PREFIX));
            assert!(invoker.handles(&definition.name));
            assert!(!crate::services::mcp::is_mcp_tool_name(&definition.name));
        }
        assert!(!invoker.handles("mcp__files__read"));
    }
}
