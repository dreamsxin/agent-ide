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
pub const RUN_COMMAND: &str = "workspace_run_command";

/// 单个文件最多回传的字节数，避免一次调用就吃掉整个上下文预算
const MAX_READ_BYTES: usize = 64_000;
/// 搜索最多回传的匹配行数
const MAX_SEARCH_RESULTS: usize = 60;
/// 列目录最多回传的条目数
const MAX_LIST_ENTRIES: usize = 200;
/// 命令输出回传给模型的字符上限。保尾部：报错在末尾
const MAX_COMMAND_OUTPUT_CHARS: usize = 12_000;
/// 遍历时跳过的目录：构建产物和依赖树，不是源码
const SKIPPED_DIRS: [&str; 6] = [
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".agent-ide",
];

/// 一次运行里内置工具的授权范围。
///
/// 只读工具无条件启用（受工作区边界约束、拒绝凭据文件）。命令执行不一样：
/// 它是真正的副作用，所以清单为空时这个工具**根本不出现在模型的工具列表里** ——
/// 而不是出现之后再拒绝。让模型看见一个永远会失败的工具只会浪费轮次。
#[derive(Clone, Debug, Default)]
pub struct WorkspaceToolPermissions {
    /// 允许执行的命令，支持 `cargo *` 前缀通配。空 = 不暴露命令执行工具
    pub allowed_commands: Vec<String>,
}

impl WorkspaceToolPermissions {
    pub fn read_only() -> Self {
        Self::default()
    }

    pub fn with_commands(allowed_commands: Vec<String>) -> Self {
        Self { allowed_commands }
    }

    fn allows_commands(&self) -> bool {
        !self.allowed_commands.is_empty()
    }
}

pub fn tool_definitions(permissions: &WorkspaceToolPermissions) -> Vec<ToolDefinition> {
    let mut definitions = vec![
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
    ];

    if permissions.allows_commands() {
        definitions.push(ToolDefinition {
            name: RUN_COMMAND.to_string(),
            description: format!(
                "Run one of the project's own check commands in the workspace root and return its \
                 exit code and output. Use this to see whether a change actually works before \
                 proposing it, and to read real failure output instead of guessing. Only these \
                 commands are permitted: {}. The command must exit on its own; dev servers and \
                 watch tasks are refused.",
                permissions.allowed_commands.join(", ")
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Exact command to run, e.g. npm test"
                    }
                },
                "required": ["command"]
            }),
        });
    }

    definitions
}

/// 工具调用日志回调：`(level, summary, details)`。
///
/// agent 层不认识 Tauri —— 发事件是命令层的事。之前这里直接持有 `AppHandle`，
/// 结果把 Tauri runtime 拖进了 lib 测试二进制，整个测试套件在加载时就
/// STATUS_ENTRYPOINT_NOT_FOUND 起不来。用回调把边界划回去，和 orchestrator
/// 只做状态、命令层负责发事件的分法一致。
pub type ToolCallLogger = std::sync::Arc<dyn Fn(&str, &str, &str) + Send + Sync>;

pub struct WorkspaceToolInvoker {
    logger: Option<ToolCallLogger>,
    permissions: WorkspaceToolPermissions,
}

impl WorkspaceToolInvoker {
    pub fn new(logger: ToolCallLogger, permissions: WorkspaceToolPermissions) -> Self {
        Self {
            logger: Some(logger),
            permissions,
        }
    }

    /// 不写日志的构造方式（测试路径）
    pub fn without_logging(permissions: WorkspaceToolPermissions) -> Self {
        Self {
            logger: None,
            permissions,
        }
    }

    /// 每次工具调用都记一条。
    ///
    /// 不记的话这些工具是完全不透明的：Agent 读了哪些文件、搜了什么，用户
    /// 无从得知，而 MCP 工具是有记录的 —— 内置工具不该比外部工具更不可审计。
    fn log(&self, level: &str, summary: &str, details: &str) {
        if let Some(ref logger) = self.logger {
            logger(level, summary, details);
        }
    }
}

#[async_trait]
impl ToolInvoker for WorkspaceToolInvoker {
    fn handles(&self, tool_name: &str) -> bool {
        match tool_name {
            READ_FILE | SEARCH_TEXT | LIST_FILES => true,
            // 未授权时不认领：工具本来也没有被通告出去，认领它只会把一个
            // "不存在的工具"变成一个"总是失败的工具"
            RUN_COMMAND => self.permissions.allows_commands(),
            _ => false,
        }
    }

    async fn invoke(&self, tool_name: &str, arguments: &str) -> Result<String, String> {
        let args: serde_json::Value = serde_json::from_str(arguments.trim())
            .map_err(|error| format!("Tool arguments are not valid JSON: {}", error))?;
        // 参数里只有路径和查询串，没有机密可言，可以原样记录
        self.log(
            "info",
            &format!("Agent called {}", tool_name),
            &format!("Arguments:\n{}", arguments.trim()),
        );
        let result = match tool_name {
            READ_FILE => read_file_tool(string_arg(&args, "path").ok_or("Missing 'path'")?),
            SEARCH_TEXT => search_text_tool(
                string_arg(&args, "query").ok_or("Missing 'query'")?,
                string_arg(&args, "extension"),
            ),
            LIST_FILES => list_files_tool(string_arg(&args, "path").unwrap_or(".")),
            RUN_COMMAND => {
                run_command_tool(
                    string_arg(&args, "command").ok_or("Missing 'command'")?,
                    &self.permissions.allowed_commands,
                )
                .await
            }
            other => Err(format!("Unknown workspace tool: {}", other)),
        };
        match &result {
            Ok(output) => self.log(
                "success",
                &format!("{} returned {} chars", tool_name, output.len()),
                output,
            ),
            Err(error) => self.log("warn", &format!("{} failed", tool_name), error),
        }
        result
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

/// 跑一条项目自己的检查命令，把退出码和输出回给模型。
///
/// 这是"闭环"缺的那一半：在此之前模型只能提出一个 diff，然后永远看不到结果，
/// 失败原因要等用户手动点一次 Verify 再贴回来。
///
/// 三重约束，顺序有意为之：
/// 1. 长驻命令一律拒绝。验证是跑完再看结果，`npm run dev` 永远不退出 ——
///    这是安全不变量，不由允许清单覆盖，所以先判它。
/// 2. 必须命中允许清单。清单由后端从项目自己声明的任务里推导，不是模型自选。
/// 3. 输出保尾部截断：报错在末尾，保头部等于只把编译进度喂给模型。
async fn run_command_tool(command: &str, allowed: &[String]) -> Result<String, String> {
    use crate::services::verification;

    if verification::is_long_running_command(command) {
        return Err(format!(
            "Refusing to run {:?}: it looks like a long-running command (dev server or watch \
             task) and would never exit. Run a check that terminates, such as a test or build \
             command.",
            command
        ));
    }
    if !verification::is_command_allowed(command, allowed) {
        return Err(format!(
            "Command {:?} is not authorized for this run. Allowed: {}",
            command,
            allowed.join(", ")
        ));
    }

    let root = workspace::workspace_root()?;
    let result =
        crate::services::project_tasks::run_project_command(command.to_string(), root).await?;
    let output = [result.stdout.as_str(), result.stderr.as_str()]
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let exit = result
        .exit_code
        .map(|code| code.to_string())
        // 退出码拿不到不等于成功：命令没能正常结束就是没结束
        .unwrap_or_else(|| "unknown (command did not report an exit code)".to_string());

    Ok(format!(
        "$ {}\nexit code: {}\nduration: {} ms\nproblems parsed: {}\n\n{}",
        result.command,
        exit,
        result.duration_ms,
        result.problems.len(),
        if output.trim().is_empty() {
            "(no output)".to_string()
        } else {
            verification::truncate_for_prompt(&output, MAX_COMMAND_OUTPUT_CHARS)
        }
    ))
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
/// 只读工具无条件启用：它们受工作区边界约束、且拒绝凭据文件，不像 MCP 那样
/// 需要用户先信任一个外部进程。命令执行按 `permissions` 决定是否暴露。
pub fn attach_workspace_tools(
    llm: crate::services::llm_client::LlmClient,
    existing: Option<std::sync::Arc<dyn ToolInvoker>>,
    logger: Option<ToolCallLogger>,
    permissions: WorkspaceToolPermissions,
) -> (
    crate::services::llm_client::LlmClient,
    Option<std::sync::Arc<dyn ToolInvoker>>,
) {
    let mut definitions = llm.extra_tools().to_vec();
    definitions.extend(tool_definitions(&permissions));

    let invoker = match logger {
        Some(logger) => WorkspaceToolInvoker::new(logger, permissions),
        None => WorkspaceToolInvoker::without_logging(permissions),
    };
    let mut invokers: Vec<std::sync::Arc<dyn ToolInvoker>> = vec![std::sync::Arc::new(invoker)];
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
        let permissions = WorkspaceToolPermissions::with_commands(vec!["npm test".to_string()]);
        let invoker = WorkspaceToolInvoker::without_logging(permissions.clone());
        for definition in tool_definitions(&permissions) {
            assert!(definition.name.starts_with(WORKSPACE_TOOL_PREFIX));
            assert!(invoker.handles(&definition.name));
            assert!(!crate::services::mcp::is_mcp_tool_name(&definition.name));
        }
        assert!(!invoker.handles("mcp__files__read"));
    }

    /// 未授权时命令工具不该出现在工具列表里，也不该被认领。
    ///
    /// 通告一个必然失败的工具比不通告更糟：模型会去调它，浪费一轮，然后才
    /// 从错误里学到它用不了。
    #[test]
    fn command_tool_is_absent_without_permission() {
        let read_only = WorkspaceToolPermissions::read_only();
        let names: Vec<String> = tool_definitions(&read_only)
            .into_iter()
            .map(|definition| definition.name)
            .collect();

        assert!(!names.contains(&RUN_COMMAND.to_string()), "{:?}", names);
        assert!(!WorkspaceToolInvoker::without_logging(read_only).handles(RUN_COMMAND));

        let with_commands = WorkspaceToolPermissions::with_commands(vec!["cargo test".to_string()]);
        let names: Vec<String> = tool_definitions(&with_commands)
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        assert!(names.contains(&RUN_COMMAND.to_string()));
        assert!(WorkspaceToolInvoker::without_logging(with_commands).handles(RUN_COMMAND));
    }

    /// 允许清单之外的命令必须拒绝，长驻命令即使在清单里也必须拒绝。
    ///
    /// 后者是安全不变量而不是偏好：验证是跑完再看结果，`npm run dev` 不会退出，
    /// 放行它等于挂住整个 stage 直到取消。
    #[tokio::test]
    async fn command_tool_enforces_allow_list_and_refuses_long_running() {
        let allowed = vec!["npm test".to_string(), "cargo *".to_string()];

        let error = run_command_tool("rm -rf /", &allowed).await.unwrap_err();
        assert!(error.contains("not authorized"), "{}", error);

        // 前缀通配命中，但这是长驻命令 —— 先判长驻，所以给出的是长驻的理由
        let error = run_command_tool("cargo watch -x test", &allowed)
            .await
            .unwrap_err();
        assert!(error.contains("long-running"), "{}", error);

        // 清单里写了也不行：长驻判定不受清单覆盖
        let error = run_command_tool("npm run dev", &["npm run dev".to_string()])
            .await
            .unwrap_err();
        assert!(error.contains("long-running"), "{}", error);
    }

    /// 命令跑完之后，退出码、耗时和输出都要回给模型 —— 这是"闭环"的关键：
    /// 没有真实输出，模型只能猜自己的改动有没有生效。
    ///
    /// 用 `block_on` 而不是 `#[tokio::test]`：`env_test_guard` 是同步锁，
    /// 在 async 测试里跨 await 持有它会触发 `await_holding_lock`，而这个守卫
    /// 保护的正是被调用方要读的 `AGENT_IDE_CONFIG_DIR`，不能提前放掉。
    #[test]
    fn command_tool_reports_exit_code_and_output() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write("src/app.ts", "");

        let command = if cfg!(windows) {
            "cmd /C exit 3"
        } else {
            "sh -c 'exit 3'"
        };
        let output = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run_command_tool(command, &[command.to_string()]))
            .unwrap();

        assert!(output.contains("exit code: 3"), "{}", output);
        assert!(output.contains("duration:"), "{}", output);
    }
}
