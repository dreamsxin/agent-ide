use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeCapability {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeBootstrap {
    pub app_name: &'static str,
    pub runtime: &'static str,
    pub capabilities: Vec<RuntimeCapability>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceEntry {
    pub path: String,
    pub name: String,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceState {
    pub root: String,
    pub entries: Vec<WorkspaceEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileDocument {
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveFileRequest {
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitState {
    pub branch: String,
    pub dirty: bool,
    pub summary: String,
    pub changes: Vec<GitChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitChange {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandRequest {
    pub command: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandResult {
    pub execution_id: String,
    pub command: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandEvent {
    pub execution_id: String,
    pub stream: &'static str,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandStarted {
    pub execution_id: String,
    pub command: String,
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionStarted {
    pub id: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionFinished {
    pub id: String,
    pub command: String,
    pub success: bool,
    pub exit_code: Option<i32>,
}

// Protocol-facing descriptor for commands the runtime can recommend for a workspace.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceTask {
    pub id: &'static str,
    pub label: &'static str,
    pub command: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentTaskRequest {
    pub goal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlan {
    pub id: String,
    pub goal: String,
    pub summary: String,
    pub source: String,
    pub steps: Vec<AgentPlanStep>,
    pub verification: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentPlanSummary {
    pub id: String,
    pub goal: String,
    pub step_count: usize,
    pub done_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlanStep {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub status: String,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProviderConfig {
    pub provider: String,
    pub model: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentProviderStatus {
    pub provider: String,
    pub model: String,
    pub configured: bool,
    pub secret_configured: bool,
    pub mode: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentProviderSecretRequest {
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPlanningLog {
    pub provider: String,
    pub model: String,
    pub goal: String,
    pub used_secret: bool,
    pub outcome: String,
}

pub fn bootstrap() -> RuntimeBootstrap {
    RuntimeBootstrap {
        app_name: "Agent IDE",
        runtime: "rust",
        capabilities: vec![
            RuntimeCapability {
                id: "workspace.open",
                label: "Open workspace",
            },
            RuntimeCapability {
                id: "workspace.read",
                label: "Read files",
            },
            RuntimeCapability {
                id: "workspace.write",
                label: "Write files",
            },
            RuntimeCapability {
                id: "git.status",
                label: "Inspect Git status",
            },
            RuntimeCapability {
                id: "command.run",
                label: "Run workspace command",
            },
            RuntimeCapability {
                id: "test.run",
                label: "Run workspace tests",
            },
            RuntimeCapability {
                id: "agent.run",
                label: "Run agent task",
            },
        ],
    }
}

pub fn open_workspace(root: &str) -> Result<WorkspaceState, String> {
    let root_path = PathBuf::from(root);
    if !root_path.is_dir() {
        return Err("selected path is not a directory".into());
    }

    let mut entries = Vec::new();
    for entry in WalkDir::new(&root_path)
        .min_depth(1)
        .max_depth(3)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if should_skip(path) {
            continue;
        }

        let relative = match path.strip_prefix(&root_path) {
            Ok(relative) => relative,
            Err(_) => continue,
        };

        entries.push(WorkspaceEntry {
            path: normalize_separators(relative),
            name: entry.file_name().to_string_lossy().into_owned(),
            kind: if path.is_dir() { "directory" } else { "file" },
        });
    }

    Ok(WorkspaceState {
        root: root_path.to_string_lossy().into_owned(),
        entries,
    })
}

pub fn read_file(root: &str, relative_path: &str) -> Result<FileDocument, String> {
    let full_path = resolve_workspace_path(root, relative_path)?;
    let contents = fs::read_to_string(&full_path).map_err(|err| err.to_string())?;

    Ok(FileDocument {
        path: normalize_separators(Path::new(relative_path)),
        contents,
    })
}

pub fn save_file(root: &str, request: SaveFileRequest) -> Result<(), String> {
    let full_path = resolve_workspace_path(root, &request.path)?;
    fs::write(full_path, request.contents).map_err(|err| err.to_string())
}

pub fn git_state(root: &str) -> GitState {
    let branch = git_output(root, ["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|| "no-git".into());
    let status = git_output(root, ["status", "--short"]).unwrap_or_default();
    let changes = parse_git_changes(&status);
    let dirty = !status.trim().is_empty();
    let summary = if branch == "no-git" {
        "Git repository not detected".into()
    } else if dirty {
        format!("{} changed item(s)", changes.len())
    } else {
        "Working tree clean".into()
    };

    GitState {
        branch,
        dirty,
        summary,
        changes,
    }
}

pub fn workspace_tasks(root: &str) -> Vec<WorkspaceTask> {
    let root_path = PathBuf::from(root);
    if !root_path.is_dir() {
        return Vec::new();
    }

    let mut tasks = Vec::new();

    if root_path.join("Cargo.toml").is_file() {
        tasks.push(WorkspaceTask {
            id: "rust.check",
            label: "Run Check",
            command: "cargo check".into(),
        });
        tasks.push(WorkspaceTask {
            id: "rust.test",
            label: "Run Tests",
            command: "cargo test".into(),
        });
    }

    if root_path.join("package.json").is_file() {
        tasks.push(WorkspaceTask {
            id: "node.test",
            label: "Run npm test",
            command: "npm test".into(),
        });
    }

    tasks
}

pub fn decompose_agent_task(root: &str, request: AgentTaskRequest) -> Result<AgentPlan, String> {
    let goal = request.goal.trim();
    if goal.is_empty() {
        return Err("agent task goal is empty".into());
    }

    let root_path = PathBuf::from(root);
    if !root_path.is_dir() {
        return Err("workspace root is not a directory".into());
    }

    let provider = agent_provider_status(root)?;
    let provider_backed = provider.provider != "local"
        && provider.mode == "provider-backed"
        && provider.secret_configured;
    let tasks = workspace_tasks(root);
    let primary_command = tasks.first().map(|task| task.command.clone());
    let git = git_state(root);
    let has_rust = root_path.join("Cargo.toml").is_file();
    let has_node = root_path.join("package.json").is_file();

    let mut steps = vec![
        AgentPlanStep {
            id: "step-1".into(),
            title: "Clarify the requested outcome".into(),
            detail: if provider_backed {
                format!("Provider `{}` will refine the requested outcome while preserving the local safety plan: {goal}", provider.provider)
            } else {
                format!("Restate the goal and identify the smallest safe implementation slice: {goal}")
            },
            status: "pending".into(),
            command: None,
        },
        AgentPlanStep {
            id: "step-2".into(),
            title: "Inspect the workspace context".into(),
            detail: describe_workspace_context(has_rust, has_node, &git),
            status: "pending".into(),
            command: Some("git status --short".into()),
        },
        AgentPlanStep {
            id: "step-3".into(),
            title: "Plan code changes behind the runtime boundary".into(),
            detail: "Keep trusted filesystem, Git, command, and Agent orchestration behavior in Rust runtime modules; keep the client presentation-oriented.".into(),
            status: "pending".into(),
            command: None,
        },
        AgentPlanStep {
            id: "step-4".into(),
            title: "Implement the smallest useful patch".into(),
            detail: "Apply the minimal code and UI changes needed for the requested behavior, preserving the runnable desktop IDE loop.".into(),
            status: "pending".into(),
            command: None,
        },
        AgentPlanStep {
            id: "step-5".into(),
            title: "Verify and summarize".into(),
            detail: "Run available checks, capture failures as follow-up work, and update V3 docs if boundaries changed.".into(),
            status: "pending".into(),
            command: primary_command,
        },
    ];

    if git.dirty {
        steps.insert(
            2,
            AgentPlanStep {
                id: "step-safety".into(),
                title: "Protect existing worktree changes".into(),
                detail: format!(
                    "The current Git state is dirty ({}). Avoid reverting unrelated user changes.",
                    git.summary
                ),
                status: "pending".into(),
                command: Some("git status --short".into()),
            },
        );
    }

    let verification = if tasks.is_empty() {
        vec!["Run targeted manual verification for this workspace.".into()]
    } else {
        tasks
            .into_iter()
            .map(|task| format!("{}: {}", task.label, task.command))
            .collect()
    };

    let plan = AgentPlan {
        id: create_execution_id().replace("exec", "plan"),
        goal: goal.into(),
        summary: if provider_backed {
            format!(
                "Provider-backed planning scaffold selected `{}` with model `{}`. Network execution is still gated behind the runtime adapter; local safety steps remain active.",
                provider.provider, provider.model
            )
        } else {
            "Local runtime generated an implementation plan scaffold. A future provider-backed Agent can refine and execute these steps.".into()
        },
        source: if provider_backed {
            provider.provider.clone()
        } else {
            "local".into()
        },
        steps,
        verification,
    };

    write_provider_planning_log(
        root,
        ProviderPlanningLog {
            provider: provider.provider,
            model: provider.model,
            goal: goal.into(),
            used_secret: provider.secret_configured,
            outcome: if provider_backed {
                "provider-backed scaffold selected; request body redacted".into()
            } else {
                "local fallback scaffold selected".into()
            },
        },
    )?;

    Ok(plan)
}

pub fn save_agent_plan(root: &str, plan: &AgentPlan) -> Result<(), String> {
    let plans_dir = agent_plans_dir(root)?;
    fs::create_dir_all(&plans_dir).map_err(|err| err.to_string())?;
    let path = plans_dir.join(format!("{}.json", plan.id));
    let contents = serde_json::to_string_pretty(plan).map_err(|err| err.to_string())?;
    fs::write(path, contents).map_err(|err| err.to_string())
}

pub fn latest_agent_plan(root: &str) -> Result<Option<AgentPlan>, String> {
    let plans_dir = agent_plans_dir(root)?;
    if !plans_dir.is_dir() {
        return Ok(None);
    }

    let mut latest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(plans_dir).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|err| err.to_string())?;
        if latest
            .as_ref()
            .map(|(current, _)| modified > *current)
            .unwrap_or(true)
        {
            latest = Some((modified, path));
        }
    }

    let Some((_, path)) = latest else {
        return Ok(None);
    };

    let contents = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&contents)
        .map(Some)
        .map_err(|err| err.to_string())
}

pub fn agent_plan_history(root: &str) -> Result<Vec<AgentPlanSummary>, String> {
    let plans_dir = agent_plans_dir(root)?;
    if !plans_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    for entry in fs::read_dir(plans_dir).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        let contents = fs::read_to_string(path).map_err(|err| err.to_string())?;
        let plan: AgentPlan = serde_json::from_str(&contents).map_err(|err| err.to_string())?;
        plans.push(AgentPlanSummary {
            id: plan.id,
            goal: plan.goal,
            step_count: plan.steps.len(),
            done_count: plan.steps.iter().filter(|step| step.status == "done").count(),
        });
    }

    plans.sort_by(|left, right| right.id.cmp(&left.id));
    Ok(plans)
}

pub fn read_agent_plan(root: &str, plan_id: &str) -> Result<AgentPlan, String> {
    if !is_safe_plan_id(plan_id) {
        return Err("invalid agent plan id".into());
    }

    let path = agent_plans_dir(root)?.join(format!("{plan_id}.json"));
    let contents = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&contents).map_err(|err| err.to_string())
}

pub fn agent_provider_status(root: &str) -> Result<AgentProviderStatus, String> {
    let config = read_agent_provider_config(root)?.unwrap_or_else(default_provider_config);
    let secret_configured = provider_secret_path(root)?.is_file();
    let configured = config.provider == "local" || secret_configured;
    let message = if configured {
        if config.provider == "local" {
            "Using local runtime decomposition fallback.".into()
        } else {
            format!("Provider `{}` has a runtime-owned secret configured.", config.provider)
        }
    } else {
        format!(
            "Provider `{}` is selected but no runtime-owned secret is configured yet.",
            config.provider
        )
    };

    Ok(AgentProviderStatus {
        provider: config.provider,
        model: config.model,
        configured,
        secret_configured,
        mode: config.mode,
        message,
    })
}

pub fn save_agent_provider_config(
    root: &str,
    config: AgentProviderConfig,
) -> Result<AgentProviderStatus, String> {
    let config_dir = agent_config_dir(root)?;
    fs::create_dir_all(&config_dir).map_err(|err| err.to_string())?;
    let contents = serde_json::to_string_pretty(&config).map_err(|err| err.to_string())?;
    fs::write(config_dir.join("provider.json"), contents).map_err(|err| err.to_string())?;
    agent_provider_status(root)
}

pub fn save_agent_provider_secret(
    root: &str,
    request: AgentProviderSecretRequest,
) -> Result<AgentProviderStatus, String> {
    let secret = request.secret.trim();
    if secret.is_empty() {
        return Err("provider secret is empty".into());
    }

    let path = provider_secret_path(root)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(path, secret).map_err(|err| err.to_string())?;
    agent_provider_status(root)
}

pub fn clear_agent_provider_secret(root: &str) -> Result<AgentProviderStatus, String> {
    let path = provider_secret_path(root)?;
    if path.is_file() {
        fs::remove_file(path).map_err(|err| err.to_string())?;
    }
    agent_provider_status(root)
}

fn write_provider_planning_log(root: &str, entry: ProviderPlanningLog) -> Result<(), String> {
    let log_dir = agent_config_dir(root)?.join("provider-logs");
    fs::create_dir_all(&log_dir).map_err(|err| err.to_string())?;
    let path = log_dir.join(format!("{}.json", create_execution_id().replace("exec", "provider")));
    let contents = serde_json::to_string_pretty(&entry).map_err(|err| err.to_string())?;
    fs::write(path, contents).map_err(|err| err.to_string())
}

pub fn run_command(root: &str, request: CommandRequest) -> Result<CommandResult, String> {
    let workspace_root = PathBuf::from(root);
    if !workspace_root.is_dir() {
        return Err("workspace root is not a directory".into());
    }

    let output = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", request.command.as_str()])
            .current_dir(&workspace_root)
            .output()
    } else {
        Command::new("sh")
            .args(["-lc", request.command.as_str()])
            .current_dir(&workspace_root)
            .output()
    }
    .map_err(|err| err.to_string())?;

    Ok(CommandResult {
        execution_id: create_execution_id(),
        command: request.command,
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

pub fn run_command_streaming<F>(
    root: &str,
    request: CommandRequest,
    mut on_started: impl FnMut(CommandStarted),
    mut on_event: F,
) -> Result<CommandResult, String>
where
    F: FnMut(CommandEvent),
{
    let execution_id = create_execution_id();
    let workspace_root = PathBuf::from(root);
    if !workspace_root.is_dir() {
        return Err("workspace root is not a directory".into());
    }

    let mut command = if cfg!(target_os = "windows") {
        let mut inner = Command::new("cmd");
        inner.args(["/C", request.command.as_str()]);
        inner
    } else {
        let mut inner = Command::new("sh");
        inner.args(["-lc", request.command.as_str()]);
        inner
    };

    let mut child = command
        .current_dir(&workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    let child_id = child.id();

    on_started(CommandStarted {
        execution_id: execution_id.clone(),
        command: request.command.clone(),
        pid: child_id,
    });

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to capture stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture stderr".to_string())?;

    let (sender, receiver) = mpsc::channel::<CommandEvent>();

    let stdout_execution_id = execution_id.clone();
    let stdout_sender = sender.clone();
    let stdout_thread = thread::spawn(move || -> Result<(), String> {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = line.map_err(|err| err.to_string())?;
            stdout_sender
                .send(CommandEvent {
                    execution_id: stdout_execution_id.clone(),
                    stream: "stdout",
                    line,
                })
                .map_err(|err| err.to_string())?;
        }
        Ok(())
    });

    let stderr_execution_id = execution_id.clone();
    let stderr_sender = sender.clone();
    let stderr_thread = thread::spawn(move || -> Result<(), String> {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let line = line.map_err(|err| err.to_string())?;
            stderr_sender
                .send(CommandEvent {
                    execution_id: stderr_execution_id.clone(),
                    stream: "stderr",
                    line,
                })
                .map_err(|err| err.to_string())?;
        }
        Ok(())
    });

    drop(sender);

    let mut stdout_lines = Vec::new();
    let mut stderr_lines = Vec::new();

    for event in receiver {
        if event.stream == "stdout" {
            stdout_lines.push(event.line.clone());
        } else {
            stderr_lines.push(event.line.clone());
        }
        on_event(event);
    }

    stdout_thread
        .join()
        .map_err(|_| "stdout worker panicked".to_string())??;
    stderr_thread
        .join()
        .map_err(|_| "stderr worker panicked".to_string())??;

    let status = child.wait().map_err(|err| err.to_string())?;

    Ok(CommandResult {
        execution_id,
        command: request.command,
        success: status.success(),
        exit_code: status.code(),
        stdout: stdout_lines.join("\n"),
        stderr: stderr_lines.join("\n"),
    })
}

pub fn cancel_process(pid: u32) -> Result<(), String> {
    let status = if cfg!(target_os = "windows") {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
    } else {
        Command::new("kill").args(["-TERM", &pid.to_string()]).status()
    }
    .map_err(|err| err.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("cancel command exited with {}", status))
    }
}

fn parse_git_changes(status: &str) -> Vec<GitChange> {
    status
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }

            let status_code = line[..2].trim().to_string();
            let raw_path = line[3..].trim();
            let path = raw_path
                .split(" -> ")
                .last()
                .unwrap_or(raw_path)
                .replace('\\', "/");

            Some(GitChange {
                path,
                status: if status_code.is_empty() {
                    "modified".into()
                } else {
                    status_code
                },
            })
        })
        .collect()
}

fn git_output<const N: usize>(root: &str, args: [&str; N]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn resolve_workspace_path(root: &str, relative_path: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(root);
    let candidate = root.join(relative_path);
    let canonical_root = root.canonicalize().map_err(|err| err.to_string())?;
    let canonical_candidate = candidate.canonicalize().map_err(|err| err.to_string())?;

    if !canonical_candidate.starts_with(&canonical_root) {
        return Err("path escapes workspace root".into());
    }

    Ok(canonical_candidate)
}

fn agent_plans_dir(root: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(root);
    let canonical_root = root.canonicalize().map_err(|err| err.to_string())?;
    Ok(canonical_root.join(".agent-ide").join("plans"))
}

fn agent_config_dir(root: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(root);
    let canonical_root = root.canonicalize().map_err(|err| err.to_string())?;
    Ok(canonical_root.join(".agent-ide"))
}

fn provider_secret_path(root: &str) -> Result<PathBuf, String> {
    Ok(agent_config_dir(root)?.join("secrets").join("provider.secret"))
}

fn normalize_separators(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn should_skip(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(name.as_ref(), ".git" | "node_modules" | "target" | "dist")
    })
}

fn is_safe_plan_id(plan_id: &str) -> bool {
    !plan_id.is_empty()
        && plan_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn read_agent_provider_config(root: &str) -> Result<Option<AgentProviderConfig>, String> {
    let path = agent_config_dir(root)?.join("provider.json");
    if !path.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&contents)
        .map(Some)
        .map_err(|err| err.to_string())
}

fn default_provider_config() -> AgentProviderConfig {
    AgentProviderConfig {
        provider: "local".into(),
        model: "local-decomposition".into(),
        mode: "offline".into(),
    }
}

fn describe_workspace_context(has_rust: bool, has_node: bool, git: &GitState) -> String {
    let stack = match (has_rust, has_node) {
        (true, true) => "Rust and Node/TypeScript workspace",
        (true, false) => "Rust workspace",
        (false, true) => "Node/TypeScript workspace",
        (false, false) => "generic workspace",
    };

    format!(
        "Detected a {stack}. Git branch is `{}` with status: {}.",
        git.branch, git.summary
    )
}

fn create_execution_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("exec-{millis:x}")
}
