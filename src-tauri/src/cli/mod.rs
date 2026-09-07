use crate::agent::diff_apply::apply_pending_diffs;
use crate::agent::planner;
use crate::agent::state_machine::{ApplyDiffsResult, FileDiff, TaskStep};
use crate::services::agent_runtime;
use crate::services::context::{
    ContextBuildOptions, ContextCompressionMode, ContextEstimateResponse, ContextSourceOptions,
};
use crate::services::llm_client::{LlmClient, LlmConfig};
use crate::services::llm_profiles;
use crate::services::problem_parser::ProblemEntry;
use crate::services::project_tasks::{self, RunProjectTaskResult};
// 修复循环的措辞和截断规则和桌面端共用，避免两套实现慢慢漂移
use crate::services::verification::{
    build_repair_prompt, collect_command_problems, failed_command_results, is_command_allowed,
};
use crate::services::{context::AgentContext, workspace};
use chrono::Utc;
use clap::error::ErrorKind;
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::cell::RefCell;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Success = 0,
    InternalError = 1,
    InvalidInput = 2,
    ChangesProposed = 3,
    ChecksFailed = 4,
    ApplyFailed = 5,
    ProviderFailed = 6,
    PreconditionFailed = 7,
    Cancelled = 8,
}

impl ExitCode {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "agent-cli",
    about = "Agent IDE headless automation CLI",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Subcommand, Debug)]
enum CliCommand {
    /// Validate local CLI prerequisites.
    Doctor(DoctorArgs),
    /// Context utilities.
    Context(ContextArgs),
    /// Generate a plan only.
    Plan(AgentCommandArgs),
    /// Run the Agent. This is also the default command when no subcommand is used.
    Run(AgentCommandArgs),
    /// Run deterministic backend smoke workflows for IDE integration paths.
    Smoke(SmokeArgs),
}

#[derive(Args, Debug)]
struct DoctorArgs {
    #[arg(long)]
    workspace: Option<PathBuf>,

    #[arg(long, value_enum, default_value = "text")]
    output: OutputMode,

    #[arg(long)]
    artifact_dir: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct ContextArgs {
    #[command(subcommand)]
    command: ContextCommand,
}

#[derive(Subcommand, Debug)]
enum ContextCommand {
    /// Estimate context sections and token budget.
    Estimate(ContextEstimateArgs),
}

#[derive(Args, Debug, Clone)]
struct ContextEstimateArgs {
    #[arg(long)]
    workspace: Option<PathBuf>,

    #[arg(long, value_enum, default_value = "focused")]
    context_mode: ContextModeArg,

    #[arg(long, value_delimiter = ',')]
    include: Vec<ContextSourceArg>,

    #[arg(long, value_enum, default_value = "text")]
    output: OutputMode,

    #[arg(long)]
    artifact_dir: Option<PathBuf>,

    #[arg(long)]
    run_id: Option<String>,
}

#[derive(Args, Debug, Clone)]
struct RunArgs {
    #[arg(long)]
    profile: Option<String>,

    #[arg(long)]
    endpoint: Option<String>,

    #[arg(long)]
    api_key: Option<String>,

    #[arg(long)]
    model: Option<String>,

    #[arg(long)]
    workspace: Option<PathBuf>,

    #[arg(long)]
    apply: bool,

    #[arg(long, value_enum, default_value = "focused")]
    context_mode: ContextModeArg,

    #[arg(long, value_delimiter = ',')]
    include: Vec<ContextSourceArg>,

    #[arg(long, value_enum, default_value = "text")]
    output: OutputMode,

    #[arg(long)]
    artifact_dir: Option<PathBuf>,

    #[arg(long)]
    run_id: Option<String>,

    #[arg(long)]
    prompt_file: Option<PathBuf>,

    #[arg(long)]
    stdin: bool,

    #[arg(long = "run-command")]
    run_commands: Vec<String>,

    #[arg(long = "allow-run")]
    allow_run: Vec<String>,

    #[arg(long, default_value_t = 0)]
    max_iterations: u8,

    #[arg(long)]
    timeout_seconds: Option<u64>,

    #[arg(long)]
    max_output_bytes: Option<usize>,

    #[arg(long)]
    max_diff_files: Option<usize>,

    #[arg(long = "deny-path")]
    deny_paths: Vec<String>,

    #[arg(long = "allow-create")]
    allow_create: bool,

    #[arg(long = "allow-edit")]
    allow_edit: bool,

    #[arg(long = "allow-delete")]
    allow_delete: bool,

    /// Expose `workspace_write_file` to the model. Requires `--apply`.
    ///
    /// 单独一个开关，不复用 `--allow-edit`：那个管的是"产出的 diff 能否落盘"，
    /// 而这个是"模型可以在运行途中直接写文件"，两件事。
    #[arg(long = "allow-agent-write")]
    allow_agent_write: bool,

    #[arg(long = "allow-git")]
    allow_git: Vec<String>,
}

#[derive(Args, Debug, Clone)]
struct AgentCommandArgs {
    #[command(flatten)]
    run: RunArgs,
    #[arg(value_name = "PROMPT", num_args = 0..)]
    prompt: Vec<String>,
}

#[derive(Args, Debug)]
struct SmokeArgs {
    #[command(subcommand)]
    command: SmokeCommand,
}

#[derive(Subcommand, Debug)]
// `IdeBackend` 带着完整的 run 参数，`IdeSurface` 只有几个字段，体积差被 clippy
// 盯上了。这里无所谓：整个进程只从 argv 构造一次这个枚举，既不放进集合也不
// 高频传递。装箱要么 clap 的 derive 不接受，要么只是为了让 lint 闭嘴。
#[allow(clippy::large_enum_variant)]
enum SmokeCommand {
    /// Exercise workspace resolution, project scripts, command checks, Problems, diff apply, and repair-chain artifacts.
    IdeBackend(AgentCommandArgs),
    /// Probe the IDE panel backends read-only: workspace, project tasks, Git, context packing.
    IdeSurface(IdeSurfaceArgs),
}

/// `smoke ide-surface` 的参数。
///
/// 刻意只读、也不需要 LLM：它回答的是"桌面端那些面板背后的后端此刻能不能工作"，
/// 而这些以前只能靠打开应用一个个点。没有 provider 依赖意味着它能进 CI。
#[derive(Args, Debug, Clone)]
struct IdeSurfaceArgs {
    #[arg(long)]
    workspace: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = OutputMode::Text)]
    output: OutputMode,

    #[arg(long)]
    artifact_dir: Option<PathBuf>,

    #[arg(long)]
    run_id: Option<String>,

    /// Context compression mode used for the context-packing probe.
    #[arg(long, value_enum, default_value_t = ContextModeArg::Budgeted)]
    context_mode: ContextModeArg,
}

impl Default for RunArgs {
    fn default() -> Self {
        Self {
            endpoint: None,
            api_key: None,
            model: None,
            profile: None,
            workspace: None,
            apply: false,
            context_mode: ContextModeArg::Focused,
            include: Vec::new(),
            output: OutputMode::Text,
            artifact_dir: None,
            run_id: None,
            prompt_file: None,
            stdin: false,
            run_commands: Vec::new(),
            allow_run: Vec::new(),
            max_iterations: 0,
            timeout_seconds: None,
            max_output_bytes: None,
            max_diff_files: None,
            deny_paths: Vec::new(),
            allow_create: false,
            allow_edit: false,
            allow_delete: false,
            allow_agent_write: false,
            allow_git: Vec::new(),
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum OutputMode {
    Text,
    Json,
    Ndjson,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum ContextModeArg {
    Full,
    Focused,
    Compact,
    Budgeted,
}

impl From<ContextModeArg> for ContextCompressionMode {
    fn from(value: ContextModeArg) -> Self {
        match value {
            ContextModeArg::Full => ContextCompressionMode::Full,
            ContextModeArg::Focused => ContextCompressionMode::Focused,
            ContextModeArg::Compact => ContextCompressionMode::Compact,
            ContextModeArg::Budgeted => ContextCompressionMode::Budgeted,
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum ContextSourceArg {
    GitDiff,
    ProjectTree,
    ProjectMemory,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum CliStatus {
    Ok,
    PlanReady,
    ChangesProposed,
    Applied,
    ApplyFailed,
    ProviderFailed,
    PreconditionFailed,
    ChecksFailed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliSummary {
    schema_version: u32,
    run_id: String,
    status: CliStatus,
    exit_code: u8,
    workspace: String,
    command: String,
    prompt: Option<String>,
    output: OutputMode,
    artifact_dir: String,
    context: Option<ContextEstimateResponse>,
    plan: Vec<TaskStep>,
    diffs: Vec<FileDiff>,
    apply_result: Option<ApplyDiffsResult>,
    commands: Vec<RunProjectTaskResult>,
    problems: Vec<ProblemEntry>,
    repair_chain: Vec<RepairIterationRecord>,
    repair_summary: Vec<RepairIterationSummary>,
    project_tasks: Vec<project_tasks::ProjectTask>,
    capabilities: Option<CliCapabilities>,
    policy: CliPolicySummary,
    errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliCapabilities {
    stable_contract: bool,
    subcommands: Vec<String>,
    output_modes: Vec<String>,
    context_modes: Vec<String>,
    artifacts: Vec<String>,
    supports_profiles: bool,
    supports_context_estimate: bool,
    supports_run_command_checks: bool,
    supports_bounded_repair: bool,
    supports_run_allow_list: bool,
    supports_timeout_policy: bool,
    supports_output_limit: bool,
    supports_diff_file_limit: bool,
    supports_compact_ci_summary: bool,
    supports_ide_backend_smoke: bool,
    supports_file_permission_policy: bool,
    supports_path_deny_policy: bool,
    supports_git_permission_policy: bool,
    supports_interactive_review: bool,
    supports_git_mutation: bool,
    scope: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepairIterationRecord {
    iteration: u8,
    prompt: String,
    failed_commands_before: Vec<RunProjectTaskResult>,
    problems_before: Vec<ProblemEntry>,
    diffs: Vec<FileDiff>,
    apply_result: ApplyDiffsResult,
    commands_after: Vec<RunProjectTaskResult>,
    checks_failed_after: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepairIterationSummary {
    iteration: u8,
    failed_commands_before: usize,
    problem_count_before: usize,
    diff_count: usize,
    applied_count: usize,
    apply_failed_count: usize,
    rerun_commands: usize,
    rerun_failed_commands: usize,
    checks_failed_after: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CliEvent {
    RunStarted {
        run_id: String,
        command: String,
        workspace: String,
    },
    ContextEstimated {
        estimate: ContextEstimateResponse,
    },
    PlanReady {
        step_count: usize,
        steps: Vec<TaskStep>,
    },
    StepStarted {
        step_id: String,
        title: String,
    },
    StepFinished {
        step_id: String,
        response_chars: usize,
        diff_count: usize,
    },
    DiffsReady {
        diff_count: usize,
    },
    ApplyFinished {
        applied_count: usize,
        failed_count: usize,
    },
    CommandFinished {
        command: String,
        exit_code: Option<i32>,
        duration_ms: u128,
    },
    RepairIterationStarted {
        iteration: u8,
        max_iterations: u8,
        problem_count: usize,
    },
    RepairIterationFinished {
        iteration: u8,
        diff_count: usize,
        checks_failed: bool,
    },
    RunFinished {
        status: CliStatus,
        exit_code: u8,
    },
    SurfaceProbed {
        name: String,
        status: String,
        detail: String,
    },
}

/// 一次 IDE 后端面探测的结果。
///
/// `unavailable` 和 `failed` 必须分开：工作区不是 git 仓库时 Git 面板本来就
/// 没东西可显示，那不是缺陷；而 `git_status` 真的报错是缺陷。混成一个状态会
/// 让这个命令在非 git 目录里永远是红的，很快就没人看了。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SurfaceProbe {
    name: String,
    status: String,
    detail: String,
}

impl SurfaceProbe {
    fn ok(name: &str, detail: String) -> Self {
        Self {
            name: name.to_string(),
            status: "ok".to_string(),
            detail,
        }
    }

    fn unavailable(name: &str, detail: String) -> Self {
        Self {
            name: name.to_string(),
            status: "unavailable".to_string(),
            detail,
        }
    }

    fn failed(name: &str, detail: String) -> Self {
        Self {
            name: name.to_string(),
            status: "failed".to_string(),
            detail,
        }
    }

    fn is_failure(&self) -> bool {
        self.status == "failed"
    }
}

/// Git 探测里哪些错误属于"这个仓库现在没东西可看"而不是缺陷。
///
/// 只剩一种：根本不是 git 仓库。这个探测命令当初还发现了第二种——刚 `git init`
/// 还没有提交时 `git_status` 直接返回 Err（git2 的 `UnbornBranch`），桌面端 Git
/// 面板在新建仓库里会因此报错。那个缺陷已经在 `commands::git` 里修掉了，所以这里
/// 不再容忍它：继续容忍等于把回归静音成"不可用"。
fn git_probe_unavailable(error: &str) -> bool {
    error.contains("Not a git repo")
}

struct CliOutput {
    mode: OutputMode,
    events: Vec<CliEvent>,
}

impl CliOutput {
    fn new(mode: OutputMode) -> Self {
        Self {
            mode,
            events: Vec::new(),
        }
    }

    fn event(&mut self, event: CliEvent) {
        if self.mode == OutputMode::Ndjson {
            if let Ok(line) = serde_json::to_string(&event) {
                println!("{}", line);
            }
        }
        self.events.push(event);
    }

    fn text(&self, message: impl AsRef<str>) {
        if self.mode == OutputMode::Text {
            println!("{}", message.as_ref());
        }
    }

    fn token_sender(&self) -> mpsc::Sender<String> {
        let (tx, mut rx) = mpsc::channel::<String>(128);
        let mode = self.mode;
        tokio::spawn(async move {
            while let Some(token) = rx.recv().await {
                if mode == OutputMode::Text {
                    print!("{}", token);
                }
            }
        });
        tx
    }
}

pub async fn run_from_env() -> u8 {
    match run_from_args(normalize_legacy_args(std::env::args())).await {
        Ok(code) => code.as_u8(),
        Err((code, message)) => {
            if code == ExitCode::Success {
                print!("{}", message);
            } else {
                eprintln!("{}", message);
            }
            code.as_u8()
        }
    }
}

async fn run_from_args<I, T>(args: I) -> Result<ExitCode, (ExitCode, String)>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).map_err(|err| {
        let rendered = err.render().ansi().to_string();
        match err.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => (ExitCode::Success, rendered),
            _ => (ExitCode::InvalidInput, rendered),
        }
    })?;

    match cli.command {
        Some(CliCommand::Doctor(args)) => run_doctor(args).await,
        Some(CliCommand::Context(args)) => run_context(args).await,
        Some(CliCommand::Plan(args)) => {
            let mut run = args.run;
            run.apply = false;
            run_agent_command("plan", run, args.prompt).await
        }
        Some(CliCommand::Run(args)) => run_agent_command("run", args.run, args.prompt).await,
        Some(CliCommand::Smoke(args)) => run_smoke(args).await,
        None => Err((
            ExitCode::InvalidInput,
            "No command provided. Use run, plan, context estimate, or doctor.".to_string(),
        )),
    }
}

fn normalize_legacy_args<I, T>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw: Vec<String> = args
        .into_iter()
        .map(|arg| arg.into().to_string_lossy().to_string())
        .collect();
    if raw.len() <= 1 {
        return raw;
    }
    if raw.iter().skip(1).any(|arg| {
        matches!(
            arg.as_str(),
            "doctor"
                | "context"
                | "plan"
                | "run"
                | "smoke"
                | "help"
                | "--help"
                | "-h"
                | "--version"
                | "-V"
        )
    }) {
        return raw;
    }
    let mut normalized = Vec::with_capacity(raw.len() + 1);
    normalized.push(raw[0].clone());
    normalized.push("run".to_string());
    normalized.extend(raw.into_iter().skip(1));
    normalized
}

async fn run_doctor(args: DoctorArgs) -> Result<ExitCode, (ExitCode, String)> {
    let mut output = CliOutput::new(args.output);
    let run_id = make_run_id(args.workspace.as_deref());
    let workspace_path = resolve_workspace(args.workspace.as_deref())?;
    let artifact_dir = args
        .artifact_dir
        .clone()
        .unwrap_or_else(|| default_artifact_dir(&workspace_path, &run_id));
    let workspace_display = workspace_path.to_string_lossy().to_string();
    let mut errors = Vec::new();

    if git2::Repository::discover(&workspace_path).is_err() {
        errors.push("Git repository was not found from workspace.".to_string());
    }

    let endpoint = std::env::var("LLM_ENDPOINT").unwrap_or_default();
    let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
    let model = std::env::var("LLM_MODEL").unwrap_or_default();
    if endpoint.is_empty() || api_key.is_empty() || model.is_empty() {
        errors.push("LLM_ENDPOINT, LLM_API_KEY, and LLM_MODEL are not all configured.".to_string());
    }

    let status = if errors.is_empty() {
        CliStatus::Ok
    } else {
        CliStatus::PreconditionFailed
    };
    let exit = if errors.is_empty() {
        ExitCode::Success
    } else {
        ExitCode::PreconditionFailed
    };

    let summary = CliSummary {
        schema_version: 1,
        run_id,
        status: status.clone(),
        exit_code: exit.as_u8(),
        workspace: workspace_display,
        command: "doctor".to_string(),
        prompt: None,
        output: args.output,
        artifact_dir: artifact_dir.to_string_lossy().to_string(),
        context: None,
        plan: Vec::new(),
        diffs: Vec::new(),
        apply_result: None,
        commands: Vec::new(),
        problems: Vec::new(),
        repair_chain: Vec::new(),
        repair_summary: Vec::new(),
        project_tasks: Vec::new(),
        capabilities: Some(cli_capabilities()),
        policy: default_policy_summary(),
        errors,
    };

    emit_summary(&mut output, &summary)?;
    write_artifacts(&artifact_dir, &summary, &output.events, None, None)?;
    Ok(exit)
}

async fn run_context(args: ContextArgs) -> Result<ExitCode, (ExitCode, String)> {
    match args.command {
        ContextCommand::Estimate(args) => run_context_estimate(args).await,
    }
}

async fn run_context_estimate(args: ContextEstimateArgs) -> Result<ExitCode, (ExitCode, String)> {
    let mut output = CliOutput::new(args.output);
    let run_id = args
        .run_id
        .clone()
        .unwrap_or_else(|| make_run_id(args.workspace.as_deref()));
    let workspace_path = resolve_workspace(args.workspace.as_deref())?;
    configure_workspace(&workspace_path)?;
    let artifact_dir = args
        .artifact_dir
        .clone()
        .unwrap_or_else(|| default_artifact_dir(&workspace_path, &run_id));
    let context = build_workspace_context(&workspace_path, &args.include);
    let estimate = estimate_context(&context, args.context_mode);

    output.event(CliEvent::RunStarted {
        run_id: run_id.clone(),
        command: "context estimate".to_string(),
        workspace: workspace_path.to_string_lossy().to_string(),
    });
    output.event(CliEvent::ContextEstimated {
        estimate: estimate.clone(),
    });

    let summary = CliSummary {
        schema_version: 1,
        run_id,
        status: CliStatus::Ok,
        exit_code: ExitCode::Success.as_u8(),
        workspace: workspace_path.to_string_lossy().to_string(),
        command: "context estimate".to_string(),
        prompt: None,
        output: args.output,
        artifact_dir: artifact_dir.to_string_lossy().to_string(),
        context: Some(estimate),
        plan: Vec::new(),
        diffs: Vec::new(),
        apply_result: None,
        commands: Vec::new(),
        problems: Vec::new(),
        repair_chain: Vec::new(),
        repair_summary: Vec::new(),
        project_tasks: Vec::new(),
        capabilities: None,
        policy: default_policy_summary(),
        errors: Vec::new(),
    };
    emit_summary(&mut output, &summary)?;
    write_artifacts(&artifact_dir, &summary, &output.events, None, None)?;
    Ok(ExitCode::Success)
}

async fn run_agent_command(
    command: &str,
    args: RunArgs,
    positional_prompt: Vec<String>,
) -> Result<ExitCode, (ExitCode, String)> {
    let mut output = CliOutput::new(args.output);
    let run_id = args
        .run_id
        .clone()
        .unwrap_or_else(|| make_run_id(args.workspace.as_deref()));
    let workspace_path = resolve_workspace(args.workspace.as_deref())?;
    configure_workspace(&workspace_path)?;
    let project_tasks = project_tasks::discover_project_tasks_in_root(&workspace_path)
        .map_err(|err| (ExitCode::InternalError, err))?;
    let artifact_dir = args
        .artifact_dir
        .clone()
        .unwrap_or_else(|| default_artifact_dir(&workspace_path, &run_id));
    let prompt = read_prompt(&args, positional_prompt)?;
    validate_repair_permissions(&args)?;
    validate_agent_write_permission(&args)?;
    let llm = build_llm_client(&args)?;
    // CLI 的工具面。在此之前 CLI 传的是 `None` invoker，所以 headless 运行里模型是
    // "盲"的 —— 这也是 CLI 需要 `--max-iterations` 修复循环兜底的原因。
    //
    // 两个开关是分开的，故意的：
    // - `--allow-run` 给验证工具，限定在同一批 pattern 内
    // - `--allow-agent-write` 给写入工具，且要求 `--apply`（见
    //   `validate_agent_write_permission`）
    //
    // 不复用 `--allow-edit` / `--allow-create`：那两个管的是"产出的 diff 能否落盘"，
    // 把它们重新解释成"模型可以直接写文件"是偷偷提权。
    let tool_permissions = crate::agent::workspace_tools::WorkspaceToolPermissions::new(
        args.allow_run.clone(),
        args.allow_agent_write,
        args.allow_agent_write && args.allow_create,
    );
    let expose_tools = !args.allow_run.is_empty() || args.allow_agent_write;
    let (llm, tool_invoker) = if expose_tools {
        crate::agent::workspace_tools::attach_workspace_tools(
            llm,
            None,
            None,
            tool_permissions.clone(),
        )
    } else {
        (llm, None)
    };
    let mut context = build_workspace_context(&workspace_path, &args.include);
    context.enrich_from_workspace_with_sources(&source_options(&args.include));
    let context_options = ContextBuildOptions::new(args.context_mode.into(), None);
    let context_text = context.to_prompt_context_with_options(&context_options);
    let context_estimate = context.estimate_prompt_context(&context_options);
    let cancel_flag = Arc::new(AtomicBool::new(false));

    output.event(CliEvent::RunStarted {
        run_id: run_id.clone(),
        command: command.to_string(),
        workspace: workspace_path.to_string_lossy().to_string(),
    });
    output.event(CliEvent::ContextEstimated {
        estimate: context_estimate.clone(),
    });
    output.text(format!("=== Agent IDE CLI ({}) ===", command));
    output.text(format!("Run ID:    {}", run_id));
    output.text(format!("Workspace: {}", workspace_path.display()));
    output.text(format!(
        "Mode:      {}",
        if args.apply { "apply" } else { "preview" }
    ));

    let plan_tx = output.token_sender();
    output.text("--- Planning ---");
    let (steps, _planner_response) = match planner::plan_task(
        &llm,
        &prompt,
        &context_text,
        cancel_flag.clone(),
        plan_tx,
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            let summary = error_summary(
                run_id,
                command,
                &workspace_path,
                args.output,
                &artifact_dir,
                prompt,
                Some(context_estimate),
                ExitCode::ProviderFailed,
                CliStatus::ProviderFailed,
                err,
            );
            emit_summary(&mut output, &summary)?;
            write_artifacts(
                &artifact_dir,
                &summary,
                &output.events,
                Some(&context_text),
                None,
            )?;
            return Ok(ExitCode::ProviderFailed);
        }
    };
    output.text("");
    output.event(CliEvent::PlanReady {
        step_count: steps.len(),
        steps: steps.clone(),
    });

    if command == "plan" {
        let status = CliStatus::PlanReady;
        let summary = CliSummary {
            schema_version: 1,
            run_id,
            status: status.clone(),
            exit_code: ExitCode::Success.as_u8(),
            workspace: workspace_path.to_string_lossy().to_string(),
            command: command.to_string(),
            prompt: Some(prompt),
            output: args.output,
            artifact_dir: artifact_dir.to_string_lossy().to_string(),
            context: Some(context_estimate),
            plan: steps,
            diffs: Vec::new(),
            apply_result: None,
            commands: Vec::new(),
            problems: Vec::new(),
            repair_chain: Vec::new(),
            repair_summary: Vec::new(),
            project_tasks,
            capabilities: None,
            policy: cli_policy_summary(&args, Vec::new()),
            errors: Vec::new(),
        };
        output.event(CliEvent::RunFinished {
            status,
            exit_code: ExitCode::Success.as_u8(),
        });
        emit_summary(&mut output, &summary)?;
        write_artifacts(
            &artifact_dir,
            &summary,
            &output.events,
            Some(&context_text),
            None,
        )?;
        return Ok(ExitCode::Success);
    }

    let mut diffs = execute_steps(
        &llm,
        &prompt,
        &context_text,
        &workspace_path,
        &steps,
        &mut output,
        cancel_flag,
        args.timeout_seconds,
        tool_invoker.clone(),
    )
    .await?;
    validate_diff_limit(&diffs, args.max_diff_files)?;
    let policy_before_apply = validate_cli_policy(&args, &diffs)?;
    output.event(CliEvent::DiffsReady {
        diff_count: diffs.len(),
    });

    let apply_result = if args.apply {
        let result = apply_pending_diffs(&diffs);
        output.event(CliEvent::ApplyFinished {
            applied_count: result.applied.len(),
            failed_count: result.failed.len(),
        });
        Some(result)
    } else {
        None
    };
    let mut apply_results = Vec::new();
    if let Some(result) = apply_result.clone() {
        apply_results.push(result);
    }

    let mut command_results = run_cli_checks(&args, &workspace_path, &mut output).await?;
    trim_command_outputs(&mut command_results, args.max_output_bytes);
    let mut checks_failed = command_results
        .iter()
        .any(|result| result.exit_code.unwrap_or(-1) != 0);
    let mut repair_chain = Vec::new();

    // 循环的准入和退出规则现在归 `verification::RepairPolicy`，桌面端要走同一套
    // （9.0.10）。这里只剩调用序列。
    let repair_policy =
        crate::services::verification::RepairPolicy::new(args.max_iterations, args.apply);
    let mut completed_iterations = 0u8;
    let mut repair_apply_failed = false;
    loop {
        let decision = repair_policy.next(completed_iterations, checks_failed, repair_apply_failed);
        let iteration = match decision {
            crate::services::verification::RepairDecision::Repair { iteration } => iteration,
            crate::services::verification::RepairDecision::Stop(stop) => {
                // 只在真的修过的时候说明为什么停：一次没修过的运行里，
                // "repair not enabled" 是噪音
                if completed_iterations > 0 {
                    output.text(format!(
                        "--- Repair stopped after {} iteration(s): {} ---",
                        completed_iterations,
                        stop.reason()
                    ));
                }
                break;
            }
        };
        let command_problems = collect_command_problems(&command_results);
        let failed_commands_before = failed_command_results(&command_results);
        output.event(CliEvent::RepairIterationStarted {
            iteration,
            max_iterations: args.max_iterations,
            problem_count: command_problems.len(),
        });
        output.text(format!(
            "--- Repair iteration {}/{} ---",
            iteration, args.max_iterations
        ));
        let repair_prompt =
            build_repair_prompt(&prompt, iteration, &command_results, &command_problems);
        let repair_steps = vec![TaskStep {
            id: format!("repair-{}-{}", iteration, Uuid::new_v4()),
            title: format!("Repair failed checks iteration {}", iteration),
            step_type: "edit".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
            scope: Some("workspace".to_string()),
            execution_mode: Some("fix".to_string()),
        }];
        let repair_diffs = execute_steps(
            &llm,
            &repair_prompt,
            &context_text,
            &workspace_path,
            &repair_steps,
            &mut output,
            Arc::new(AtomicBool::new(false)),
            args.timeout_seconds,
            tool_invoker.clone(),
        )
        .await?;
        validate_diff_limit(&repair_diffs, args.max_diff_files)?;
        let _ = validate_cli_policy(&args, &repair_diffs)?;
        let repair_apply = apply_pending_diffs(&repair_diffs);
        output.event(CliEvent::ApplyFinished {
            applied_count: repair_apply.applied.len(),
            failed_count: repair_apply.failed.len(),
        });
        diffs.extend(repair_diffs.clone());
        apply_results.push(repair_apply.clone());
        command_results = run_cli_checks(&args, &workspace_path, &mut output).await?;
        trim_command_outputs(&mut command_results, args.max_output_bytes);
        checks_failed = command_results
            .iter()
            .any(|result| result.exit_code.unwrap_or(-1) != 0);
        repair_chain.push(RepairIterationRecord {
            iteration,
            prompt: repair_prompt,
            failed_commands_before,
            problems_before: command_problems,
            diffs: repair_diffs,
            apply_result: repair_apply.clone(),
            commands_after: command_results.clone(),
            checks_failed_after: checks_failed,
        });
        output.event(CliEvent::RepairIterationFinished {
            iteration,
            diff_count: diffs.len(),
            checks_failed,
        });
        repair_apply_failed = !repair_apply.failed.is_empty();
        completed_iterations = iteration;
    }

    let command_problems = collect_all_observed_problems(&command_results, &repair_chain);
    let apply_result = merge_apply_results(apply_results);
    let exit = if checks_failed {
        ExitCode::ChecksFailed
    } else if let Some(result) = &apply_result {
        if result.failed.is_empty() {
            ExitCode::Success
        } else {
            ExitCode::ApplyFailed
        }
    } else if diffs.is_empty() {
        ExitCode::Success
    } else {
        ExitCode::ChangesProposed
    };
    let status = match exit {
        ExitCode::Success if args.apply => CliStatus::Applied,
        ExitCode::Success => CliStatus::Ok,
        ExitCode::ChangesProposed => CliStatus::ChangesProposed,
        ExitCode::ChecksFailed => CliStatus::ChecksFailed,
        ExitCode::ApplyFailed => CliStatus::ApplyFailed,
        _ => CliStatus::Ok,
    };

    output.event(CliEvent::RunFinished {
        status: status.clone(),
        exit_code: exit.as_u8(),
    });

    let errors: Vec<String> = apply_result
        .as_ref()
        .map(|result| {
            result
                .failed
                .iter()
                .map(|failure| format!("{}: {}", failure.file, failure.message))
                .collect()
        })
        .unwrap_or_default();
    let mut errors = errors;
    for result in &command_results {
        if result.exit_code.unwrap_or(-1) != 0 {
            errors.push(format!(
                "Command failed (exit {}): {}",
                result
                    .exit_code
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                result.command
            ));
        }
    }

    let summary = CliSummary {
        schema_version: 1,
        run_id,
        status,
        exit_code: exit.as_u8(),
        workspace: workspace_path.to_string_lossy().to_string(),
        command: command.to_string(),
        prompt: Some(prompt),
        output: args.output,
        artifact_dir: artifact_dir.to_string_lossy().to_string(),
        context: Some(context_estimate),
        plan: steps,
        diffs: diffs.clone(),
        apply_result,
        commands: command_results,
        problems: command_problems,
        repair_summary: repair_chain_summary(&repair_chain),
        repair_chain,
        project_tasks,
        capabilities: None,
        policy: cli_policy_summary(&args, policy_before_apply),
        errors,
    };
    emit_summary(&mut output, &summary)?;
    write_artifacts(
        &artifact_dir,
        &summary,
        &output.events,
        Some(&context_text),
        Some(&diffs),
    )?;
    // 工具写入要留痕。桌面端把它们登记成可撤销的 applied diff 卡片，CLI 没有审查区，
    // 所以对应物是 run artifact：不写这一份，磁盘变了而运行记录里查不到是谁改的。
    //
    // 只要开了写权限就落这份文件，哪怕是空数组：文件缺失和"一次都没写"是两件事，
    // 混在一起的话读运行记录的人分不出"没授权"和"授权了但没用"。
    if args.allow_agent_write {
        let records: Vec<serde_json::Value> = tool_permissions
            .take_writes()
            .iter()
            .map(|write| {
                serde_json::json!({
                    "file": write.file,
                    "created": write.previous.is_none(),
                    "previousChars": write.previous.as_ref().map(|value| value.len()),
                    "updatedChars": write.updated.len(),
                })
            })
            .collect();
        write_json(artifact_dir.join("tool-writes.json"), &records)?;
    }
    Ok(exit)
}

async fn run_smoke(args: SmokeArgs) -> Result<ExitCode, (ExitCode, String)> {
    match args.command {
        SmokeCommand::IdeBackend(args) => run_ide_backend_smoke(args).await,
        SmokeCommand::IdeSurface(args) => run_ide_surface_smoke(args).await,
    }
}

/// 只读地探一遍桌面端各面板背后的后端。
///
/// 存在的理由：`agent_cli` 一行 `commands::` 都没 import，走的全是 `services::`
/// 和 `agent::`，所以 Agent 流程之外的东西（Git 面板、命令面板、上下文装配）
/// 在自动化里完全没有入口 —— 只能打开应用一个个点。这个子命令用**桌面端调用的
/// 同一批函数**把它们跑一遍并给出机器可读结果。
///
/// 刻意不碰的东西：终端 PTY 和 LSP 需要真的起子进程，写操作和 fetch/pull/push
/// 会碰网络和磁盘。这个命令必须能在 CI 里对任意仓库无副作用地跑。
async fn run_ide_surface_smoke(args: IdeSurfaceArgs) -> Result<ExitCode, (ExitCode, String)> {
    let mut output = CliOutput::new(args.output);
    let run_id = args
        .run_id
        .clone()
        .unwrap_or_else(|| make_run_id(args.workspace.as_deref()));
    let workspace_path = resolve_workspace(args.workspace.as_deref())?;
    configure_workspace(&workspace_path)?;
    let workspace_display = workspace_path.to_string_lossy().to_string();
    let artifact_dir = args
        .artifact_dir
        .clone()
        .unwrap_or_else(|| default_artifact_dir(&workspace_path, &run_id));

    output.event(CliEvent::RunStarted {
        run_id: run_id.clone(),
        command: "smoke ide-surface".to_string(),
        workspace: workspace_display.clone(),
    });

    let mut probes = Vec::new();

    // 1. 工作区边界解析：所有面板都建立在它之上，它错了后面全错
    probes.push(match workspace::resolve_existing(".") {
        Ok(path) => SurfaceProbe::ok("workspace_resolve", path.to_string_lossy().to_string()),
        Err(error) => SurfaceProbe::failed("workspace_resolve", error),
    });

    // 2. 命令面板 / TopBar 的命令来源
    let tasks = project_tasks::discover_project_tasks_in_root(&workspace_path);
    probes.push(match &tasks {
        Ok(tasks) if tasks.is_empty() => SurfaceProbe::unavailable(
            "project_tasks",
            "No package.json scripts or Cargo tasks were discovered".to_string(),
        ),
        Ok(tasks) => SurfaceProbe::ok(
            "project_tasks",
            format!(
                "{} task(s): {}",
                tasks.len(),
                tasks
                    .iter()
                    .map(|task| task.command.as_str())
                    .take(8)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        Err(error) => SurfaceProbe::failed("project_tasks", error.clone()),
    });

    // 3. Agent 验证工具和 Verify All 实际会覆盖哪些命令。
    //    长驻命令被排除，所以这里显示的是"真的会跑的那些"，不是全部任务。
    if let Ok(tasks) = &tasks {
        let verifiable: Vec<&str> = tasks
            .iter()
            .map(|task| task.command.as_str())
            .filter(|command| !crate::services::verification::is_long_running_command(command))
            .collect();
        probes.push(if verifiable.is_empty() {
            SurfaceProbe::unavailable(
                "verification_candidates",
                "Every discovered task looks long-running, so a verification pass would cover nothing"
                    .to_string(),
            )
        } else {
            SurfaceProbe::ok(
                "verification_candidates",
                format!("{}: {}", verifiable.len(), verifiable.join(", ")),
            )
        });
    }

    // 4. Git 面板后端，调的就是桌面端那两个函数
    probes.push(
        match crate::commands::git::git_status(workspace_display.clone()) {
            Ok(status) => SurfaceProbe::ok(
                "git_status",
                format!(
                    "branch {}, {} changed entr(ies), {} staged, ahead {} behind {}, {} conflict(s)",
                    status.branch,
                    status.entries.len(),
                    status.entries.iter().filter(|entry| entry.staged).count(),
                    status.ahead,
                    status.behind,
                    status.conflicts.len()
                ),
            ),
            Err(error) if git_probe_unavailable(&error) => {
                SurfaceProbe::unavailable("git_status", error)
            }
            Err(error) => SurfaceProbe::failed("git_status", error),
        },
    );
    probes.push(
        match crate::commands::git::git_diff(
            workspace_display.clone(),
            None,
            Some("all".to_string()),
        ) {
            Ok(diff) => SurfaceProbe::ok("git_diff", format!("{} chars", diff.len())),
            Err(error) if git_probe_unavailable(&error) => {
                SurfaceProbe::unavailable("git_diff", error)
            }
            Err(error) => SurfaceProbe::failed("git_diff", error),
        },
    );

    // 5. 上下文装配：段落配额、预算裁剪，Agent 每次运行都依赖它
    let context = build_workspace_context(&workspace_path, &[]);
    let estimate = estimate_context(&context, args.context_mode);
    probes.push(SurfaceProbe::ok(
        "context_estimate",
        format!(
            "{} section(s), {} estimated tokens, budget {}",
            estimate.sections.len(),
            estimate.estimated_tokens,
            estimate
                .input_budget_tokens
                .map(|tokens| tokens.to_string())
                .unwrap_or_else(|| "unset".to_string())
        ),
    ));

    for probe in &probes {
        output.event(CliEvent::SurfaceProbed {
            name: probe.name.clone(),
            status: probe.status.clone(),
            detail: probe.detail.clone(),
        });
        output.text(format!(
            "[{}] {}: {}",
            probe.status, probe.name, probe.detail
        ));
    }

    let errors: Vec<String> = probes
        .iter()
        .filter(|probe| probe.is_failure())
        .map(|probe| format!("{}: {}", probe.name, probe.detail))
        .collect();
    let (status, exit) = if errors.is_empty() {
        (CliStatus::Ok, ExitCode::Success)
    } else {
        (CliStatus::PreconditionFailed, ExitCode::PreconditionFailed)
    };

    let summary = CliSummary {
        schema_version: 1,
        run_id,
        status: status.clone(),
        exit_code: exit.as_u8(),
        workspace: workspace_display,
        command: "smoke ide-surface".to_string(),
        prompt: None,
        output: args.output,
        artifact_dir: artifact_dir.to_string_lossy().to_string(),
        context: Some(estimate),
        plan: Vec::new(),
        diffs: Vec::new(),
        apply_result: None,
        commands: Vec::new(),
        problems: Vec::new(),
        repair_chain: Vec::new(),
        repair_summary: Vec::new(),
        project_tasks: tasks.unwrap_or_default(),
        capabilities: None,
        policy: default_policy_summary(),
        errors,
    };

    emit_summary(&mut output, &summary)?;
    write_artifacts(&artifact_dir, &summary, &output.events, None, None)?;
    // 探测结果单独落一份，方便外部工具直接读而不用从事件流里筛
    let probes_path = artifact_dir.join("surface-probes.json");
    if let Ok(serialized) = serde_json::to_string_pretty(&probes) {
        let _ = std::fs::write(probes_path, serialized);
    }
    Ok(exit)
}

async fn run_ide_backend_smoke(mut args: AgentCommandArgs) -> Result<ExitCode, (ExitCode, String)> {
    let workspace_path = resolve_workspace(args.run.workspace.as_deref())?;
    configure_workspace(&workspace_path)?;

    if args.run.run_commands.is_empty() {
        let tasks = project_tasks::discover_project_tasks_in_root(&workspace_path)
            .map_err(|err| (ExitCode::InternalError, err))?;
        let command = select_smoke_command(&tasks).ok_or_else(|| {
            (
                ExitCode::PreconditionFailed,
                "No package/Cargo test, check, lint, or build command was discovered for smoke ide-backend.".to_string(),
            )
        })?;
        args.run.run_commands.push(command);
    }

    for command in args.run.run_commands.clone() {
        if !is_command_allowed(&command, &args.run.allow_run) {
            args.run.allow_run.push(command);
        }
    }

    args.run.apply = true;
    args.run.allow_create = true;
    args.run.allow_edit = true;
    if args.run.max_iterations == 0 {
        args.run.max_iterations = 1;
    }
    if args.run.include.is_empty() {
        args.run.include = vec![
            ContextSourceArg::ProjectTree,
            ContextSourceArg::GitDiff,
            ContextSourceArg::ProjectMemory,
        ];
    }
    if args.prompt.is_empty() {
        args.prompt = vec![
            "Run the IDE backend smoke loop. Fix the failing project command while preserving behavior."
                .to_string(),
        ];
    }

    run_agent_command("smoke ide-backend", args.run, args.prompt).await
}

async fn execute_steps(
    llm: &LlmClient,
    prompt: &str,
    context_text: &str,
    workspace_path: &Path,
    steps: &[TaskStep],
    output: &mut CliOutput,
    cancel_flag: Arc<AtomicBool>,
    timeout_seconds: Option<u64>,
    tool_invoker: Option<Arc<dyn crate::agent::executor::ToolInvoker>>,
) -> Result<Vec<FileDiff>, (ExitCode, String)> {
    let output_mode = output.mode;
    let deferred_events = RefCell::new(Vec::<CliEvent>::new());
    let execution = agent_runtime::execute_agent_steps(
        llm,
        prompt,
        context_text,
        workspace_path,
        steps,
        tool_invoker.as_deref(),
        cancel_flag,
        |index, total, step| {
            if output_mode == OutputMode::Text {
                println!("--- Step {}/{}: {} ---", index + 1, total, step.title);
            }
            emit_or_defer_event(
                output_mode,
                &mut deferred_events.borrow_mut(),
                CliEvent::StepStarted {
                    step_id: step.id.clone(),
                    title: step.title.clone(),
                },
            );
        },
        |step, response, diffs| {
            if output_mode == OutputMode::Text {
                println!();
            }
            emit_or_defer_event(
                output_mode,
                &mut deferred_events.borrow_mut(),
                CliEvent::StepFinished {
                    step_id: step.id.clone(),
                    response_chars: response.chars().count(),
                    diff_count: diffs.len(),
                },
            );
        },
        |_| token_sender_for_mode(output_mode),
    );
    let results = if let Some(seconds) = timeout_seconds {
        timeout(Duration::from_secs(seconds), execution)
            .await
            .map_err(|_| {
                (
                    ExitCode::ProviderFailed,
                    format!("Agent execution timed out after {} second(s).", seconds),
                )
            })?
    } else {
        execution.await
    }
    .map_err(|err| (ExitCode::ProviderFailed, err))?;
    output.events.extend(deferred_events.into_inner());

    Ok(results
        .into_iter()
        .flat_map(|result| result.diffs)
        .collect::<Vec<_>>())
}

fn emit_or_defer_event(mode: OutputMode, events: &mut Vec<CliEvent>, event: CliEvent) {
    if mode == OutputMode::Ndjson {
        if let Ok(line) = serde_json::to_string(&event) {
            println!("{}", line);
        }
    }
    events.push(event);
}

fn token_sender_for_mode(mode: OutputMode) -> mpsc::Sender<String> {
    let (tx, mut rx) = mpsc::channel::<String>(128);
    tokio::spawn(async move {
        while let Some(token) = rx.recv().await {
            if mode == OutputMode::Text {
                print!("{}", token);
            }
        }
    });
    tx
}

fn emit_summary(output: &mut CliOutput, summary: &CliSummary) -> Result<(), (ExitCode, String)> {
    match output.mode {
        OutputMode::Json => {
            let json = serde_json::to_string_pretty(summary)
                .map_err(|err| (ExitCode::InternalError, err.to_string()))?;
            println!("{}", json);
        }
        OutputMode::Text => print_text_summary(summary),
        OutputMode::Ndjson => {}
    }
    Ok(())
}

fn print_text_summary(summary: &CliSummary) {
    println!("====================");
    println!("Status:   {:?}", summary.status);
    println!("Exit:     {}", summary.exit_code);
    println!("Run ID:   {}", summary.run_id);
    println!("Workspace:{}", summary.workspace);
    println!("Artifacts:{}", summary.artifact_dir);
    println!("Plan:     {} step(s)", summary.plan.len());
    println!("Diffs:    {} file(s)", summary.diffs.len());
    println!("Commands: {} run(s)", summary.commands.len());
    println!("Problems: {} item(s)", summary.problems.len());
    println!("Repair:   {} iteration(s)", summary.repair_chain.len());
    if let Some(result) = &summary.apply_result {
        println!(
            "Apply:    {} applied, {} failed",
            result.applied.len(),
            result.failed.len()
        );
    }
    for error in &summary.errors {
        println!("Error:    {}", error);
    }
    if !summary.repair_chain.is_empty() {
        for iteration in &summary.repair_summary {
            println!(
                "Repair #{}: before_failed={}, problems={}, diffs={}, applied={}, apply_failed={}, rerun_failed={}, checks_failed_after={}",
                iteration.iteration,
                iteration.failed_commands_before,
                iteration.problem_count_before,
                iteration.diff_count,
                iteration.applied_count,
                iteration.apply_failed_count,
                iteration.rerun_failed_commands,
                iteration.checks_failed_after
            );
        }
    }
    println!("====================");
}

fn write_artifacts(
    artifact_dir: &Path,
    summary: &CliSummary,
    events: &[CliEvent],
    context_text: Option<&str>,
    diffs: Option<&[FileDiff]>,
) -> Result<(), (ExitCode, String)> {
    fs::create_dir_all(artifact_dir).map_err(|err| {
        (
            ExitCode::InternalError,
            format!("Create artifacts: {}", err),
        )
    })?;
    write_json(artifact_dir.join("summary.json"), summary)?;
    write_json(artifact_dir.join("policy.json"), &summary.policy)?;
    write_json(artifact_dir.join("events.json"), events)?;
    let ndjson = events
        .iter()
        .filter_map(|event| serde_json::to_string(event).ok())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(artifact_dir.join("events.ndjson"), ndjson)
        .map_err(|err| (ExitCode::InternalError, format!("Write events: {}", err)))?;
    if let Some(prompt) = summary.prompt.as_deref() {
        fs::write(artifact_dir.join("prompt.txt"), prompt)
            .map_err(|err| (ExitCode::InternalError, format!("Write prompt: {}", err)))?;
    }
    if let Some(context_text) = context_text {
        fs::write(artifact_dir.join("context.txt"), context_text)
            .map_err(|err| (ExitCode::InternalError, format!("Write context: {}", err)))?;
    }
    write_json(artifact_dir.join("plan.json"), &summary.plan)?;
    if let Some(context) = &summary.context {
        write_json(artifact_dir.join("context.json"), context)?;
    }
    if let Some(diffs) = diffs {
        write_json(artifact_dir.join("changes.json"), diffs)?;
    }
    if let Some(result) = &summary.apply_result {
        write_json(artifact_dir.join("apply-result.json"), result)?;
    }
    if !summary.commands.is_empty() {
        write_json(artifact_dir.join("commands.json"), &summary.commands)?;
    }
    if !summary.project_tasks.is_empty() {
        write_json(
            artifact_dir.join("project-tasks.json"),
            &summary.project_tasks,
        )?;
    }
    if !summary.problems.is_empty() {
        write_json(artifact_dir.join("problems.json"), &summary.problems)?;
    }
    if !summary.repair_chain.is_empty() {
        write_json(
            artifact_dir.join("repair-chain.json"),
            &summary.repair_chain,
        )?;
        write_json(
            artifact_dir.join("repair-summary.json"),
            &summary.repair_summary,
        )?;
    }
    Ok(())
}

fn write_json<T: Serialize + ?Sized>(path: PathBuf, value: &T) -> Result<(), (ExitCode, String)> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|err| (ExitCode::InternalError, err.to_string()))?;
    fs::write(&path, json).map_err(|err| {
        (
            ExitCode::InternalError,
            format!("Write {}: {}", path.display(), err),
        )
    })
}

fn error_summary(
    run_id: String,
    command: &str,
    workspace_path: &Path,
    output: OutputMode,
    artifact_dir: &Path,
    prompt: String,
    context: Option<ContextEstimateResponse>,
    exit: ExitCode,
    status: CliStatus,
    error: String,
) -> CliSummary {
    CliSummary {
        schema_version: 1,
        run_id,
        status,
        exit_code: exit.as_u8(),
        workspace: workspace_path.to_string_lossy().to_string(),
        command: command.to_string(),
        prompt: Some(prompt),
        output,
        artifact_dir: artifact_dir.to_string_lossy().to_string(),
        context,
        plan: Vec::new(),
        diffs: Vec::new(),
        apply_result: None,
        commands: Vec::new(),
        problems: Vec::new(),
        repair_chain: Vec::new(),
        repair_summary: Vec::new(),
        project_tasks: Vec::new(),
        capabilities: None,
        policy: default_policy_summary(),
        errors: vec![error],
    }
}

fn cli_capabilities() -> CliCapabilities {
    CliCapabilities {
        stable_contract: true,
        subcommands: vec![
            "doctor".to_string(),
            "context estimate".to_string(),
            "plan".to_string(),
            "run".to_string(),
            "smoke ide-backend".to_string(),
        ],
        output_modes: vec!["text".to_string(), "json".to_string(), "ndjson".to_string()],
        context_modes: vec![
            "full".to_string(),
            "focused".to_string(),
            "compact".to_string(),
            "budgeted".to_string(),
        ],
        artifacts: vec![
            "summary.json".to_string(),
            "events.json".to_string(),
            "events.ndjson".to_string(),
            "policy.json".to_string(),
            "prompt.txt".to_string(),
            "context.json".to_string(),
            "context.txt".to_string(),
            "plan.json".to_string(),
            "changes.json".to_string(),
            "apply-result.json".to_string(),
            "commands.json".to_string(),
            "problems.json".to_string(),
            "repair-chain.json".to_string(),
        ],
        supports_profiles: true,
        supports_context_estimate: true,
        supports_run_command_checks: true,
        supports_bounded_repair: true,
        supports_run_allow_list: true,
        supports_timeout_policy: true,
        supports_output_limit: true,
        supports_diff_file_limit: true,
        supports_compact_ci_summary: true,
        supports_ide_backend_smoke: true,
        supports_file_permission_policy: true,
        supports_path_deny_policy: true,
        supports_git_permission_policy: true,
        supports_interactive_review: false,
        supports_git_mutation: false,
        scope: "headless automation runner; not a full command-line IDE".to_string(),
    }
}

fn select_smoke_command(tasks: &[project_tasks::ProjectTask]) -> Option<String> {
    const PREFERRED_LABELS: &[&str] = &["test", "check", "lint", "typecheck", "build"];
    for label in PREFERRED_LABELS {
        if let Some(task) = tasks
            .iter()
            .find(|task| task.label.eq_ignore_ascii_case(label))
        {
            return Some(task.command.clone());
        }
    }
    tasks
        .iter()
        .find(|task| {
            let command = task.command.to_ascii_lowercase();
            command.contains(" test")
                || command.ends_with("test")
                || command.contains(" check")
                || command.ends_with("check")
        })
        .map(|task| task.command.clone())
}

fn repair_chain_summary(chain: &[RepairIterationRecord]) -> Vec<RepairIterationSummary> {
    chain
        .iter()
        .map(|iteration| RepairIterationSummary {
            iteration: iteration.iteration,
            failed_commands_before: iteration.failed_commands_before.len(),
            problem_count_before: iteration.problems_before.len(),
            diff_count: iteration.diffs.len(),
            applied_count: iteration.apply_result.applied.len(),
            apply_failed_count: iteration.apply_result.failed.len(),
            rerun_commands: iteration.commands_after.len(),
            rerun_failed_commands: iteration
                .commands_after
                .iter()
                .filter(|command| command.exit_code.unwrap_or(-1) != 0)
                .count(),
            checks_failed_after: iteration.checks_failed_after,
        })
        .collect()
}

async fn run_cli_checks(
    args: &RunArgs,
    workspace_path: &Path,
    output: &mut CliOutput,
) -> Result<Vec<RunProjectTaskResult>, (ExitCode, String)> {
    let mut results = Vec::new();
    for command in &args.run_commands {
        let run = project_tasks::run_project_command(command.clone(), workspace_path.to_path_buf());
        let result = if let Some(seconds) = args.timeout_seconds {
            timeout(Duration::from_secs(seconds), run)
                .await
                .map_err(|_| {
                    (
                        ExitCode::ChecksFailed,
                        format!("Command timed out after {} second(s): {}", seconds, command),
                    )
                })?
        } else {
            run.await
        }
        .map_err(|err| (ExitCode::InternalError, err))?;
        output.event(CliEvent::CommandFinished {
            command: result.command.clone(),
            exit_code: result.exit_code,
            duration_ms: result.duration_ms,
        });
        results.push(result);
    }
    Ok(results)
}

fn validate_diff_limit(
    diffs: &[FileDiff],
    max_diff_files: Option<usize>,
) -> Result<(), (ExitCode, String)> {
    let Some(limit) = max_diff_files else {
        return Ok(());
    };
    if diffs.len() > limit {
        return Err((
            ExitCode::PreconditionFailed,
            format!(
                "Generated {} diff file(s), exceeding --max-diff-files {}.",
                diffs.len(),
                limit
            ),
        ));
    }
    Ok(())
}

fn validate_cli_policy(
    args: &RunArgs,
    diffs: &[FileDiff],
) -> Result<Vec<String>, (ExitCode, String)> {
    let policy = effective_policy(args);
    let mut decisions = Vec::new();
    for diff in diffs {
        let normalized_file = diff.file.replace('\\', "/");
        if let Some(pattern) = policy
            .deny_paths
            .iter()
            .find(|pattern| path_denied(&normalized_file, pattern))
        {
            return Err((
                ExitCode::PreconditionFailed,
                format!(
                    "Policy denied generated change for {} by --deny-path {}",
                    diff.file, pattern
                ),
            ));
        }
        // 用 hunk 形态判定是否新建文件，而不是信 provenance.operation：
        // `attach_step_provenance` 给后端生成的 diff 填的是 "unknown"，缺省又
        // 回落成 "edit"，于是一个真的会创建文件的 diff 会通过 --allow-edit
        // 检查、绕过 --allow-create。hunk 形态是 apply 真正分支的依据。
        let operation = if crate::agent::diff_apply::is_new_file_diff(diff) {
            "create"
        } else {
            diff.provenance
                .as_ref()
                .map(|provenance| provenance.operation.as_str())
                .unwrap_or("edit")
        };
        if !args.apply {
            decisions.push(format!("preview {} {}", operation, diff.file));
            continue;
        }
        match operation {
            "create" if !policy.allow_create => {
                return Err((
                    ExitCode::PreconditionFailed,
                    format!(
                        "Policy denied file creation for {}. Pass --allow-create to permit it.",
                        diff.file
                    ),
                ));
            }
            "delete" if !policy.allow_delete => {
                return Err((
                    ExitCode::PreconditionFailed,
                    format!(
                        "Policy denied file deletion for {}. Pass --allow-delete to permit it.",
                        diff.file
                    ),
                ));
            }
            "edit" | "unknown" if !policy.allow_edit => {
                return Err((
                    ExitCode::PreconditionFailed,
                    format!(
                        "Policy denied file edit for {}. Pass --allow-edit to permit it.",
                        diff.file
                    ),
                ));
            }
            _ => {
                decisions.push(format!("allow {} {}", operation, diff.file));
            }
        }
    }
    if !policy.allow_git.is_empty() {
        decisions.push(format!("allow git actions: {}", policy.allow_git.join(",")));
    }
    Ok(decisions)
}

fn effective_policy(args: &RunArgs) -> CliPolicySummary {
    CliPolicySummary {
        allow_create: args.allow_create || args.apply,
        allow_edit: args.allow_edit || args.apply,
        allow_delete: args.allow_delete,
        allow_git: args.allow_git.clone(),
        deny_paths: args.deny_paths.clone(),
        decisions: Vec::new(),
    }
}

fn cli_policy_summary(args: &RunArgs, decisions: Vec<String>) -> CliPolicySummary {
    CliPolicySummary {
        decisions,
        ..effective_policy(args)
    }
}

fn default_policy_summary() -> CliPolicySummary {
    CliPolicySummary {
        allow_create: false,
        allow_edit: false,
        allow_delete: false,
        allow_git: Vec::new(),
        deny_paths: Vec::new(),
        decisions: Vec::new(),
    }
}

fn path_denied(file: &str, pattern: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    if pattern.is_empty() {
        return false;
    }
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return file == prefix || file.starts_with(&format!("{}/", prefix));
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return file.starts_with(prefix);
    }
    file == pattern || file.starts_with(&format!("{}/", pattern.trim_end_matches('/')))
}

fn trim_command_outputs(results: &mut [RunProjectTaskResult], max_output_bytes: Option<usize>) {
    let Some(limit) = max_output_bytes else {
        return;
    };
    for result in results {
        result.stdout = trim_text_bytes(&result.stdout, limit);
        result.stderr = trim_text_bytes(&result.stderr, limit);
    }
}

fn trim_text_bytes(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = 0usize;
    for (index, _) in value.char_indices() {
        if index > max_bytes {
            break;
        }
        end = index;
    }
    if end == 0 {
        return "... output truncated ...".to_string();
    }
    format!("{}\n... output truncated ...", &value[..end])
}

fn collect_all_observed_problems(
    command_results: &[RunProjectTaskResult],
    repair_chain: &[RepairIterationRecord],
) -> Vec<ProblemEntry> {
    let mut problems = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for problem in collect_command_problems(command_results).into_iter().chain(
        repair_chain
            .iter()
            .flat_map(|iteration| iteration.problems_before.clone()),
    ) {
        if seen.insert(problem.id.clone()) {
            problems.push(problem);
        }
    }
    problems
}

fn merge_apply_results(results: Vec<ApplyDiffsResult>) -> Option<ApplyDiffsResult> {
    if results.is_empty() {
        return None;
    }
    let mut merged = ApplyDiffsResult {
        applied: Vec::new(),
        failed: Vec::new(),
    };
    for result in results {
        merged.applied.extend(result.applied);
        merged.failed.extend(result.failed);
    }
    Some(merged)
}

fn build_llm_client(args: &RunArgs) -> Result<LlmClient, (ExitCode, String)> {
    if let Some(profile_id) = args.profile.as_deref() {
        let config = llm_profiles::load_llm_config_from_disk().ok_or_else(|| {
            (
                ExitCode::InvalidInput,
                "No LLM profile config found. Configure profiles in the IDE or use --endpoint/--api-key/--model.".to_string(),
            )
        })?;
        let llm_config =
            llm_profiles::resolve_llm_config(&config, Some(profile_id)).map_err(|err| {
                (
                    ExitCode::InvalidInput,
                    format!("Failed to load LLM profile '{}': {}", profile_id, err),
                )
            })?;
        return Ok(LlmClient::new(llm_config));
    }

    let endpoint = args
        .endpoint
        .clone()
        .or_else(|| std::env::var("LLM_ENDPOINT").ok())
        .unwrap_or_default();
    let api_key = args
        .api_key
        .clone()
        .or_else(|| std::env::var("LLM_API_KEY").ok())
        .unwrap_or_default();
    let model = args
        .model
        .clone()
        .or_else(|| std::env::var("LLM_MODEL").ok())
        .unwrap_or_default();

    if endpoint.is_empty() || api_key.is_empty() || model.is_empty() {
        return Err((
            ExitCode::InvalidInput,
            "Missing LLM config. Provide --endpoint/--api-key/--model or LLM_ENDPOINT/LLM_API_KEY/LLM_MODEL.".to_string(),
        ));
    }

    Ok(LlmClient::new(LlmConfig {
        endpoint,
        api_key,
        model: model.clone(),
        provider: "custom".to_string(),
        max_output_tokens: None,
        // 只有真的要给工具时才切到原生工具：否则任意 provider 都会突然收到
        // `tools` 参数，而 CLI 的默认目标是"能对着任何 OpenAI 兼容端点跑"。
        tool_call_mode: if args.allow_run.is_empty() && !args.allow_agent_write {
            "text_protocol".to_string()
        } else {
            "native_tools".to_string()
        },
        model_type: crate::services::llm_client::ModelType::from_string(&model),
        local_model_config: None,
    }))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliPolicySummary {
    allow_create: bool,
    allow_edit: bool,
    allow_delete: bool,
    allow_git: Vec<String>,
    deny_paths: Vec<String>,
    decisions: Vec<String>,
}

fn read_prompt(
    args: &RunArgs,
    positional_prompt: Vec<String>,
) -> Result<String, (ExitCode, String)> {
    let mut parts = Vec::new();
    if let Some(path) = &args.prompt_file {
        parts.push(fs::read_to_string(path).map_err(|err| {
            (
                ExitCode::InvalidInput,
                format!("Read prompt file {}: {}", path.display(), err),
            )
        })?);
    }
    if args.stdin {
        let mut input = String::new();
        io::stdin()
            .read_to_string(&mut input)
            .map_err(|err| (ExitCode::InvalidInput, format!("Read stdin: {}", err)))?;
        parts.push(input);
    }
    if !positional_prompt.is_empty() {
        parts.push(positional_prompt.join(" "));
    }
    let prompt = parts.join("\n").trim().to_string();
    if prompt.is_empty() {
        Err((
            ExitCode::InvalidInput,
            "Prompt is required. Pass a prompt argument, --prompt-file, or --stdin.".to_string(),
        ))
    } else {
        Ok(prompt)
    }
}

fn validate_repair_permissions(args: &RunArgs) -> Result<(), (ExitCode, String)> {
    if args.max_iterations == 0 {
        return Ok(());
    }
    if !args.apply {
        return Err((
            ExitCode::InvalidInput,
            "--max-iterations requires --apply so generated repair diffs can be tested."
                .to_string(),
        ));
    }
    if args.run_commands.is_empty() {
        return Err((
            ExitCode::InvalidInput,
            "--max-iterations requires at least one --run-command check.".to_string(),
        ));
    }

    let unauthorized = args
        .run_commands
        .iter()
        .filter(|command| !is_command_allowed(command, &args.allow_run))
        .cloned()
        .collect::<Vec<_>>();
    if !unauthorized.is_empty() {
        return Err((
            ExitCode::InvalidInput,
            format!(
                "--max-iterations requires explicit --allow-run for command(s): {}",
                unauthorized.join(", ")
            ),
        ));
    }
    Ok(())
}

fn validate_agent_write_permission(args: &RunArgs) -> Result<(), (ExitCode, String)> {
    if !args.allow_agent_write {
        return Ok(());
    }
    // 没有 `--apply` 的运行是预览：用户的预期是"磁盘一点都不动"。写入工具会在
    // 运行途中落盘，直接违背这个预期，所以必须显式配上 `--apply`。
    // 这和桌面端把写入工具锁在 Auto 模式是同一条理由 —— Auto 本来就会自动落盘。
    if !args.apply {
        return Err((
            ExitCode::InvalidInput,
            "--allow-agent-write requires --apply: without it a run is a preview and must leave the workspace untouched, but the write tool changes files mid-run.".to_string(),
        ));
    }
    Ok(())
}

fn resolve_workspace(path: Option<&Path>) -> Result<PathBuf, (ExitCode, String)> {
    let candidate = match path {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().map_err(|err| {
            (
                ExitCode::PreconditionFailed,
                format!("Current directory is not accessible: {}", err),
            )
        })?,
    };
    match candidate.canonicalize() {
        Ok(path) if path.is_dir() => Ok(path),
        Ok(path) => Err((
            ExitCode::PreconditionFailed,
            format!("Workspace is not a directory: {}", path.display()),
        )),
        Err(err) => Err((
            ExitCode::PreconditionFailed,
            format!("Workspace is not accessible: {}", err),
        )),
    }
}

fn configure_workspace(workspace_path: &Path) -> Result<(), (ExitCode, String)> {
    std::env::set_var("AGENT_IDE_CONFIG_DIR", workspace_path.join(".agent-ide"));
    workspace::save_workspace_path(workspace_path.to_string_lossy().as_ref()).map_err(|err| {
        (
            ExitCode::PreconditionFailed,
            format!("Failed to set workspace: {}", err),
        )
    })
}

fn build_workspace_context(workspace_path: &Path, includes: &[ContextSourceArg]) -> AgentContext {
    let mut context = AgentContext::new(&workspace_path.to_string_lossy());
    context.enrich_from_workspace_with_sources(&source_options(includes));
    context
}

fn estimate_context(
    context: &AgentContext,
    context_mode: ContextModeArg,
) -> ContextEstimateResponse {
    context.estimate_prompt_context(&ContextBuildOptions::new(context_mode.into(), None))
}

fn source_options(includes: &[ContextSourceArg]) -> ContextSourceOptions {
    let include_project_tree =
        includes.contains(&ContextSourceArg::ProjectTree) || includes.is_empty();
    let include_git_diff = includes.contains(&ContextSourceArg::GitDiff) || includes.is_empty();
    let include_project_memory =
        includes.contains(&ContextSourceArg::ProjectMemory) || includes.is_empty();
    ContextSourceOptions {
        include_project_tree,
        include_git_diff,
        include_project_memory,
    }
}

fn make_run_id(workspace: Option<&Path>) -> String {
    let name = workspace
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    format!(
        "run-{}-{}-{}",
        Utc::now().format("%Y%m%d%H%M%S"),
        sanitize_run_part(name),
        &Uuid::new_v4().simple().to_string()[..8]
    )
}

fn sanitize_run_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn default_artifact_dir(workspace_path: &Path, run_id: &str) -> PathBuf {
    workspace_path.join(".agent-ide").join("runs").join(run_id)
}

#[cfg(test)]
// 这些测试用同步互斥量串行化对进程级环境变量的修改，锁必须跨 await 持有；
// 生产代码里的跨 await 持锁仍然会被 clippy 拦下。
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn cli_smoke_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct SmokeWorkspace {
        root: PathBuf,
        artifacts: PathBuf,
    }

    impl SmokeWorkspace {
        fn new(name: &str, content: &str) -> Self {
            let id = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("agent-cli-smoke-{name}-{id}"));
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join("smoke.txt"), content).unwrap();
            let repo = git2::Repository::init(&root).unwrap();
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Agent CLI Smoke").unwrap();
            config
                .set_str("user.email", "agent-cli-smoke@example.test")
                .unwrap();
            let artifacts = root.join("artifacts");
            Self { root, artifacts }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.root.join(name)
        }

        fn write(&self, name: &str, content: &str) {
            let path = self.path(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
    }

    impl Drop for SmokeWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn set_mock_llm_env() {
        std::env::set_var("LLM_ENDPOINT", "mock://cli-smoke");
        std::env::set_var("LLM_API_KEY", "sk-smoke");
        std::env::set_var("LLM_MODEL", "mock-model");
    }

    #[test]
    fn legacy_prompt_args_parse_as_run_command() {
        let normalized = normalize_legacy_args([
            "agent-cli",
            "--endpoint",
            "https://example.com/v1",
            "--api-key",
            "sk-test",
            "--model",
            "model",
            "hello",
            "world",
        ]);
        let cli = Cli::try_parse_from(normalized).unwrap();
        match cli.command {
            Some(CliCommand::Run(args)) => {
                assert_eq!(args.prompt, vec!["hello".to_string(), "world".to_string()]);
                assert_eq!(args.run.endpoint.as_deref(), Some("https://example.com/v1"));
            }
            _ => panic!("expected normalized run command"),
        }
    }

    #[test]
    fn run_subcommand_parse_json_output() {
        let cli = Cli::try_parse_from([
            "agent-cli",
            "run",
            "--output",
            "json",
            "--context-mode",
            "compact",
            "fix tests",
        ])
        .unwrap();
        match cli.command {
            Some(CliCommand::Run(args)) => {
                assert_eq!(args.run.output, OutputMode::Json);
                assert_eq!(args.run.context_mode, ContextModeArg::Compact);
                assert_eq!(args.prompt, vec!["fix tests".to_string()]);
            }
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn run_subcommand_parse_run_command_checks() {
        let cli = Cli::try_parse_from([
            "agent-cli",
            "run",
            "--run-command",
            "npm test",
            "--run-command",
            "cargo test",
            "--allow-run",
            "npm test",
            "--allow-run",
            "cargo *",
            "--max-iterations",
            "2",
            "--timeout-seconds",
            "30",
            "--max-output-bytes",
            "1024",
            "--max-diff-files",
            "5",
            "--deny-path",
            "secrets/**",
            "--allow-create",
            "--allow-edit",
            "--allow-git",
            "status",
            "--apply",
            "fix tests",
        ])
        .unwrap();
        match cli.command {
            Some(CliCommand::Run(args)) => {
                assert_eq!(args.run.run_commands, vec!["npm test", "cargo test"]);
                assert_eq!(args.run.allow_run, vec!["npm test", "cargo *"]);
                assert_eq!(args.run.max_iterations, 2);
                assert_eq!(args.run.timeout_seconds, Some(30));
                assert_eq!(args.run.max_output_bytes, Some(1024));
                assert_eq!(args.run.max_diff_files, Some(5));
                assert_eq!(args.run.deny_paths, vec!["secrets/**"]);
                assert!(args.run.allow_create);
                assert!(args.run.allow_edit);
                assert_eq!(args.run.allow_git, vec!["status"]);
                assert!(args.run.apply);
                assert_eq!(args.prompt, vec!["fix tests".to_string()]);
            }
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn repair_permissions_require_allow_run() {
        let mut args = RunArgs {
            apply: true,
            max_iterations: 1,
            run_commands: vec!["npm test".to_string()],
            ..RunArgs::default()
        };

        assert!(validate_repair_permissions(&args).is_err());

        args.allow_run = vec!["npm test".to_string()];
        assert!(validate_repair_permissions(&args).is_ok());
    }

    #[test]
    fn repair_permissions_support_prefix_wildcard() {
        assert!(is_command_allowed(
            "cargo test --all",
            &["cargo *".to_string()]
        ));
        assert!(!is_command_allowed("npm test", &["cargo *".to_string()]));
        assert!(is_command_allowed("npm run test", &["*".to_string()]));
    }

    #[test]
    fn failed_command_results_keeps_only_non_zero_checks() {
        let results = vec![
            RunProjectTaskResult {
                command: "npm test".to_string(),
                exit_code: Some(1),
                duration_ms: 1,
                stdout: String::new(),
                stderr: String::new(),
                problems: Vec::new(),
            },
            RunProjectTaskResult {
                command: "npm run lint".to_string(),
                exit_code: Some(0),
                duration_ms: 1,
                stdout: String::new(),
                stderr: String::new(),
                problems: Vec::new(),
            },
        ];

        let failed = failed_command_results(&results);

        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].command, "npm test");
    }

    #[test]
    fn trim_text_bytes_preserves_utf8_boundary() {
        let trimmed = trim_text_bytes("abcd你好", 5);

        assert!(trimmed.starts_with("abcd"));
        assert!(trimmed.contains("output truncated"));
        assert!(std::str::from_utf8(trimmed.as_bytes()).is_ok());
    }

    #[test]
    fn validate_diff_limit_rejects_too_many_files() {
        let diffs = vec![
            FileDiff {
                id: "d1".to_string(),
                file: "a.ts".to_string(),
                base_hash: None,
                provenance: None,
                hunks: Vec::new(),
                status: "pending".to_string(),
            },
            FileDiff {
                id: "d2".to_string(),
                file: "b.ts".to_string(),
                base_hash: None,
                provenance: None,
                hunks: Vec::new(),
                status: "pending".to_string(),
            },
        ];

        assert!(validate_diff_limit(&diffs, Some(1)).is_err());
        assert!(validate_diff_limit(&diffs, Some(2)).is_ok());
    }

    #[test]
    fn cli_policy_rejects_denied_paths() {
        let args = RunArgs {
            apply: true,
            deny_paths: vec!["secrets/**".to_string()],
            ..RunArgs::default()
        };
        let diffs = vec![FileDiff {
            id: "d1".to_string(),
            file: "secrets/token.txt".to_string(),
            base_hash: None,
            provenance: None,
            hunks: Vec::new(),
            status: "pending".to_string(),
        }];

        assert!(validate_cli_policy(&args, &diffs).is_err());
    }

    #[test]
    fn cli_policy_rejects_delete_during_apply_by_default() {
        let args = RunArgs {
            apply: true,
            ..RunArgs::default()
        };
        let diffs = vec![FileDiff {
            id: "d1".to_string(),
            file: "src/old.ts".to_string(),
            base_hash: None,
            provenance: Some(crate::agent::state_machine::DiffProvenance {
                protocol: "agent-changes".to_string(),
                operation: "delete".to_string(),
                rationale: None,
                schema_version: Some(1),
                change_index: Some(0),
                source_role: None,
                source_stage: None,
                regenerated_from_diff_id: None,
                regenerated_from_hunk_index: None,
            }),
            hunks: Vec::new(),
            status: "pending".to_string(),
        }];

        assert!(validate_cli_policy(&args, &diffs).is_err());
        let allowed = RunArgs {
            apply: true,
            allow_delete: true,
            ..RunArgs::default()
        };
        assert!(validate_cli_policy(&allowed, &diffs).is_ok());
    }

    /// 回归测试：新建文件的 diff 不能因为 provenance 是 "unknown" 就被记成编辑。
    /// `attach_step_provenance` 给后端生成的 diff 填的正是 "unknown"，所以这条
    /// 路径在真实 CLI 运行中会被走到，而 policy.json 的决策记录是审计依据。
    #[test]
    fn cli_policy_labels_create_shaped_diff_as_create() {
        let preview = RunArgs {
            allow_edit: true,
            ..RunArgs::default()
        };
        let create_shaped = vec![FileDiff {
            id: "d1".to_string(),
            file: "src/brand-new.ts".to_string(),
            base_hash: None,
            provenance: Some(crate::agent::state_machine::DiffProvenance {
                protocol: "unknown".to_string(),
                operation: "unknown".to_string(),
                rationale: None,
                schema_version: None,
                change_index: None,
                source_role: None,
                source_stage: None,
                regenerated_from_diff_id: None,
                regenerated_from_hunk_index: None,
            }),
            hunks: vec![crate::agent::state_machine::DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                content: String::new(),
                original: String::new(),
                updated: "export const created = true;\n".to_string(),
                provenance: None,
                status: None,
            }],
            status: "pending".to_string(),
        }];

        let decisions = validate_cli_policy(&preview, &create_shaped).unwrap();

        assert_eq!(
            decisions,
            vec!["preview create src/brand-new.ts".to_string()]
        );
    }

    #[test]
    fn repair_prompt_includes_problems_and_failed_output() {
        let problems = vec![ProblemEntry {
            id: "p1".to_string(),
            file: "src/app.ts".to_string(),
            line: 10,
            column: 5,
            severity: "error".to_string(),
            source: "typescript".to_string(),
            message: "Cannot find name value".to_string(),
        }];
        let commands = vec![RunProjectTaskResult {
            command: "npm test".to_string(),
            exit_code: Some(1),
            duration_ms: 42,
            stdout: "test failed".to_string(),
            stderr: "src/app.ts:10:5 Cannot find name value".to_string(),
            problems: problems.clone(),
        }];

        let prompt = build_repair_prompt("Fix tests", 1, &commands, &problems);

        assert!(prompt.contains("Original task:"));
        assert!(prompt.contains("Fix tests"));
        assert!(prompt.contains("src/app.ts:10:5"));
        assert!(prompt.contains("$ npm test (exit 1)"));
        assert!(prompt.contains("test failed"));
    }

    #[test]
    fn context_estimate_subcommand_parse_include_flags() {
        let cli = Cli::try_parse_from([
            "agent-cli",
            "context",
            "estimate",
            "--include",
            "git-diff,project-tree",
            "--output",
            "ndjson",
        ])
        .unwrap();
        match cli.command {
            Some(CliCommand::Context(ContextArgs {
                command: ContextCommand::Estimate(args),
            })) => {
                assert_eq!(args.output, OutputMode::Ndjson);
                assert_eq!(args.include.len(), 2);
            }
            _ => panic!("expected context estimate"),
        }
    }

    #[test]
    fn exit_codes_are_stable() {
        assert_eq!(ExitCode::Success.as_u8(), 0);
        assert_eq!(ExitCode::ChangesProposed.as_u8(), 3);
        assert_eq!(ExitCode::ApplyFailed.as_u8(), 5);
        assert_eq!(ExitCode::ProviderFailed.as_u8(), 6);
    }

    #[test]
    fn cli_capabilities_describe_closed_headless_scope() {
        let capabilities = cli_capabilities();

        assert!(capabilities.stable_contract);
        assert!(capabilities.subcommands.contains(&"run".to_string()));
        assert!(capabilities.supports_profiles);
        assert!(capabilities.supports_bounded_repair);
        assert!(capabilities.supports_timeout_policy);
        assert!(capabilities.supports_output_limit);
        assert!(capabilities.supports_diff_file_limit);
        assert!(capabilities.supports_compact_ci_summary);
        assert!(capabilities.supports_ide_backend_smoke);
        assert!(capabilities.supports_file_permission_policy);
        assert!(capabilities.supports_path_deny_policy);
        assert!(capabilities.supports_git_permission_policy);
        assert!(!capabilities.supports_interactive_review);
        assert!(!capabilities.supports_git_mutation);
        assert!(capabilities.scope.contains("headless automation"));
    }

    #[test]
    fn smoke_command_selects_package_test_before_build() {
        let tasks = vec![
            project_tasks::ProjectTask {
                id: "npm:build".to_string(),
                label: "build".to_string(),
                command: "npm run build".to_string(),
                source: "package.json".to_string(),
                description: "vite build".to_string(),
            },
            project_tasks::ProjectTask {
                id: "npm:test".to_string(),
                label: "test".to_string(),
                command: "npm run test".to_string(),
                source: "package.json".to_string(),
                description: "node test.js".to_string(),
            },
        ];

        assert_eq!(
            select_smoke_command(&tasks).as_deref(),
            Some("npm run test")
        );
    }

    #[tokio::test]
    async fn smoke_doctor_json_writes_capabilities() {
        let _guard = cli_smoke_lock()
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _workspace_guard = workspace::env_test_guard();
        set_mock_llm_env();
        let workspace = SmokeWorkspace::new("doctor", "initial");

        let exit = run_from_args([
            "agent-cli".to_string(),
            "doctor".to_string(),
            "--workspace".to_string(),
            workspace.root.to_string_lossy().to_string(),
            "--artifact-dir".to_string(),
            workspace.artifacts.to_string_lossy().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ])
        .await
        .unwrap();

        assert_eq!(exit, ExitCode::Success);
        let summary: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(workspace.artifacts.join("summary.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(summary["command"], "doctor");
        assert_eq!(summary["capabilities"]["stableContract"], true);
        assert_eq!(summary["capabilities"]["supportsBoundedRepair"], true);
    }

    #[tokio::test]
    async fn smoke_preview_writes_changes_without_applying() {
        let _guard = cli_smoke_lock()
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _workspace_guard = workspace::env_test_guard();
        set_mock_llm_env();
        let workspace = SmokeWorkspace::new("preview", "initial");

        let exit = run_from_args([
            "agent-cli".to_string(),
            "run".to_string(),
            "--workspace".to_string(),
            workspace.root.to_string_lossy().to_string(),
            "--artifact-dir".to_string(),
            workspace.artifacts.to_string_lossy().to_string(),
            "--endpoint".to_string(),
            "mock://cli-smoke".to_string(),
            "--api-key".to_string(),
            "sk-smoke".to_string(),
            "--model".to_string(),
            "mock-model".to_string(),
            "Update smoke file".to_string(),
        ])
        .await
        .unwrap();

        assert_eq!(exit, ExitCode::ChangesProposed);
        assert_eq!(
            fs::read_to_string(workspace.path("smoke.txt")).unwrap(),
            "initial"
        );
        assert!(workspace.artifacts.join("changes.json").exists());
        let summary: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(workspace.artifacts.join("summary.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(summary["status"], "changes_proposed");
    }

    #[tokio::test]
    async fn smoke_apply_writes_apply_result_and_updates_file() {
        let _guard = cli_smoke_lock()
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _workspace_guard = workspace::env_test_guard();
        set_mock_llm_env();
        let workspace = SmokeWorkspace::new("apply", "initial");

        let exit = run_from_args([
            "agent-cli".to_string(),
            "run".to_string(),
            "--workspace".to_string(),
            workspace.root.to_string_lossy().to_string(),
            "--artifact-dir".to_string(),
            workspace.artifacts.to_string_lossy().to_string(),
            "--endpoint".to_string(),
            "mock://cli-smoke".to_string(),
            "--api-key".to_string(),
            "sk-smoke".to_string(),
            "--model".to_string(),
            "mock-model".to_string(),
            "--apply".to_string(),
            "Update smoke file".to_string(),
        ])
        .await
        .unwrap();

        assert_eq!(exit, ExitCode::Success);
        assert_eq!(
            fs::read_to_string(workspace.path("smoke.txt")).unwrap(),
            "changed"
        );
        assert!(workspace.artifacts.join("apply-result.json").exists());
    }

    #[tokio::test]
    async fn smoke_repair_chain_artifact_links_failure_and_rerun() {
        let _guard = cli_smoke_lock()
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _workspace_guard = workspace::env_test_guard();
        set_mock_llm_env();
        let workspace = SmokeWorkspace::new("repair", "initial");
        let check_command = if cfg!(windows) {
            "findstr fixed smoke.txt"
        } else {
            "grep fixed smoke.txt"
        };

        let exit = run_from_args([
            "agent-cli".to_string(),
            "run".to_string(),
            "--workspace".to_string(),
            workspace.root.to_string_lossy().to_string(),
            "--artifact-dir".to_string(),
            workspace.artifacts.to_string_lossy().to_string(),
            "--endpoint".to_string(),
            "mock://cli-smoke".to_string(),
            "--api-key".to_string(),
            "sk-smoke".to_string(),
            "--model".to_string(),
            "mock-model".to_string(),
            "--apply".to_string(),
            "--run-command".to_string(),
            check_command.to_string(),
            "--allow-run".to_string(),
            check_command.to_string(),
            "--max-iterations".to_string(),
            "1".to_string(),
            "Update smoke file and repair checks".to_string(),
        ])
        .await
        .unwrap();

        assert_eq!(exit, ExitCode::Success);
        assert_eq!(
            fs::read_to_string(workspace.path("smoke.txt")).unwrap(),
            "fixed"
        );
        let chain_path = workspace.artifacts.join("repair-chain.json");
        assert!(chain_path.exists());
        let chain: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(chain_path).unwrap()).unwrap();
        assert_eq!(chain.as_array().unwrap().len(), 1);
        assert_eq!(chain[0]["checksFailedAfter"], false);
        assert!(!chain[0]["failedCommandsBefore"]
            .as_array()
            .unwrap()
            .is_empty());
        let summary_path = workspace.artifacts.join("repair-summary.json");
        assert!(summary_path.exists());
        let repair_summary: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(summary_path).unwrap()).unwrap();
        assert_eq!(repair_summary[0]["diffCount"], 1);
        assert_eq!(repair_summary[0]["rerunFailedCommands"], 0);
        let summary: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(workspace.artifacts.join("summary.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(summary["repairSummary"][0]["checksFailedAfter"], false);
    }

    #[tokio::test]
    async fn smoke_ide_backend_covers_project_command_problem_apply_and_repair_artifacts() {
        let _guard = cli_smoke_lock()
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _workspace_guard = workspace::env_test_guard();
        set_mock_llm_env();
        let workspace = SmokeWorkspace::new("ide-backend", "initial");
        workspace.write(
            "package.json",
            r#"{"scripts":{"build":"node test.js","test":"node test.js"}}"#,
        );
        // `file:///` + 绝对路径只在 Windows 上正确（根以盘符开头）。Unix 的根本身
        // 就是一个斜杠，硬拼会得到 `file:////tmp/...`。按平台拼出合法 URI。
        let workspace_uri = {
            let root = workspace.root.to_string_lossy().replace('\\', "/");
            if cfg!(windows) {
                format!("file:///{}", root)
            } else {
                format!("file://{}", root)
            }
        };
        let test_js = r#"import { readFileSync } from "node:fs";
const value = readFileSync("smoke.txt", "utf8").trim();
if (value !== "fixed") {
  console.error("ReferenceError: smoke is not fixed");
  console.error("    at WORKSPACE_URI/test.js:3:1");
  process.exit(1);
}
console.log("ok");
"#
        .replace("WORKSPACE_URI", &workspace_uri);
        workspace.write("test.js", &test_js);

        let exit = run_from_args([
            "agent-cli".to_string(),
            "smoke".to_string(),
            "ide-backend".to_string(),
            "--workspace".to_string(),
            workspace.root.to_string_lossy().to_string(),
            "--artifact-dir".to_string(),
            workspace.artifacts.to_string_lossy().to_string(),
            "--endpoint".to_string(),
            "mock://cli-smoke".to_string(),
            "--api-key".to_string(),
            "sk-smoke".to_string(),
            "--model".to_string(),
            "mock-model".to_string(),
            "--output".to_string(),
            "json".to_string(),
            "Fix backend smoke failure".to_string(),
        ])
        .await
        .unwrap();

        assert_eq!(exit, ExitCode::Success);
        assert_eq!(
            fs::read_to_string(workspace.path("smoke.txt")).unwrap(),
            "fixed"
        );

        for artifact in [
            "summary.json",
            "context.json",
            "project-tasks.json",
            "commands.json",
            "problems.json",
            "changes.json",
            "apply-result.json",
            "repair-chain.json",
            "repair-summary.json",
        ] {
            assert!(
                workspace.artifacts.join(artifact).exists(),
                "missing artifact {artifact}"
            );
        }

        let tasks: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(workspace.artifacts.join("project-tasks.json")).unwrap(),
        )
        .unwrap();
        assert!(tasks
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["command"] == "npm run test"));

        let summary: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(workspace.artifacts.join("summary.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(summary["command"], "smoke ide-backend");
        assert_eq!(summary["status"], "applied");
        assert_eq!(summary["commands"][0]["command"], "npm run test");
        assert_eq!(summary["commands"][0]["exitCode"], 0);
        assert_eq!(summary["repairSummary"][0]["problemCountBefore"], 1);
        assert_eq!(summary["repairSummary"][0]["checksFailedAfter"], false);

        let problems: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(workspace.artifacts.join("problems.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(problems.as_array().unwrap().len(), 1);
        assert_eq!(
            problems[0]["file"],
            workspace
                .path("test.js")
                .to_string_lossy()
                .replace('\\', "/")
        );
        assert_eq!(problems[0]["line"], 3);
        assert_eq!(problems[0]["column"], 1);

        let chain: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(workspace.artifacts.join("repair-chain.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(chain.as_array().unwrap().len(), 1);
        assert_eq!(
            chain[0]["failedCommandsBefore"][0]["command"],
            "npm run test"
        );
        assert!(chain[0]["prompt"]
            .as_str()
            .unwrap()
            .contains("Parsed Problems"));
        assert_eq!(
            chain[0]["applyResult"]["applied"].as_array().unwrap().len(),
            1
        );
        assert_eq!(chain[0]["commandsAfter"][0]["exitCode"], 0);
    }

    /// `smoke ide-surface` 是 Agent 流程之外那些面板后端的唯一自动化入口，
    /// 所以它必须在**没有** LLM 配置、也不管工作区是不是 git 仓库的情况下成功：
    /// 一旦它需要前置条件才能跑，就进不了 CI，也就等于不存在。
    #[tokio::test]
    async fn smoke_ide_surface_probes_panel_backends_without_a_provider() {
        let _guard = cli_smoke_lock()
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _workspace_guard = workspace::env_test_guard();
        let workspace = SmokeWorkspace::new("surface", "initial");

        let exit = run_from_args([
            "agent-cli".to_string(),
            "smoke".to_string(),
            "ide-surface".to_string(),
            "--workspace".to_string(),
            workspace.root.to_string_lossy().to_string(),
            "--artifact-dir".to_string(),
            workspace.artifacts.to_string_lossy().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ])
        .await
        .unwrap();

        let probes_raw =
            fs::read_to_string(workspace.artifacts.join("surface-probes.json")).unwrap();
        assert_eq!(exit, ExitCode::Success, "probes: {}", probes_raw);

        let probes: serde_json::Value = serde_json::from_str(&probes_raw).unwrap();
        let probes = probes.as_array().expect("probe array").clone();
        let by_name = |name: &str| {
            probes
                .iter()
                .find(|probe| probe["name"] == name)
                .unwrap_or_else(|| panic!("missing probe {}", name))
                .clone()
        };

        assert_eq!(by_name("workspace_resolve")["status"], "ok");
        assert_eq!(by_name("context_estimate")["status"], "ok");
        // git 探测在非仓库目录里必须是 unavailable 而不是 failed —— 否则这个命令
        // 在任何非 git 目录里都是红的，很快就没人看它的结果了
        let git_status = by_name("git_status")["status"].clone();
        assert!(
            git_status == "ok" || git_status == "unavailable",
            "{}",
            git_status
        );

        let summary: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(workspace.artifacts.join("summary.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(summary["status"], "ok");
        assert_eq!(summary["command"], "smoke ide-surface");
    }

    /// 工具循环此前在**任何**自动化路径里都跑不到：mock provider 只能回文本
    /// （`stream_mock_chat` 返回 `String`），CLI 又给执行器传 `None` invoker。
    /// 这是那条链路的第一份端到端证据：模型发出工具调用 → 执行器真的跑了命令 →
    /// 结果作为 `tool` 消息回填 → 下一轮产出 diff。
    ///
    /// 断言选的是"命令的副作用"而不是"这一轮完成了"：工具报错时同样会进入下一轮
    /// 并产出 diff，所以只看 diff 分不出真假。命令用 shell 重定向写文件，
    /// `run_project_command` 在两个平台上都过 shell，因此不依赖 node。
    #[tokio::test]
    async fn smoke_tool_loop_runs_an_allow_listed_command() {
        let _guard = cli_smoke_lock()
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _workspace_guard = workspace::env_test_guard();
        set_mock_llm_env();
        let workspace = SmokeWorkspace::new("toolloop", "broken");
        let command = "echo yes > tool-ran.txt";
        std::env::set_var("AGENT_IDE_MOCK_TOOL", "workspace_run_command");
        std::env::set_var(
            "AGENT_IDE_MOCK_TOOL_ARGS",
            serde_json::json!({ "command": command }).to_string(),
        );

        let result = run_from_args([
            "agent-cli".to_string(),
            "run".to_string(),
            "--workspace".to_string(),
            workspace.root.to_string_lossy().to_string(),
            "--artifact-dir".to_string(),
            workspace.artifacts.to_string_lossy().to_string(),
            "--allow-run".to_string(),
            command.to_string(),
            "Update smoke file".to_string(),
        ])
        .await;

        std::env::remove_var("AGENT_IDE_MOCK_TOOL");
        std::env::remove_var("AGENT_IDE_MOCK_TOOL_ARGS");
        let exit = result.unwrap();

        assert!(
            workspace.path("tool-ran.txt").exists(),
            "the allow-listed command never ran, so the tool loop did not complete"
        );
        assert_eq!(exit, ExitCode::ChangesProposed);
    }

    /// 未授权时命令工具不通告，所以模型即使"想"调也调不到 —— 这条约束必须在
    /// CLI 这条路径上也成立，不能只在桌面端成立。
    #[tokio::test]
    async fn smoke_tool_loop_is_absent_without_allow_run() {
        let _guard = cli_smoke_lock()
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _workspace_guard = workspace::env_test_guard();
        set_mock_llm_env();
        let workspace = SmokeWorkspace::new("notools", "broken");
        let command = "echo yes > tool-ran.txt";
        std::env::set_var("AGENT_IDE_MOCK_TOOL", "workspace_run_command");
        std::env::set_var(
            "AGENT_IDE_MOCK_TOOL_ARGS",
            serde_json::json!({ "command": command }).to_string(),
        );

        let result = run_from_args([
            "agent-cli".to_string(),
            "run".to_string(),
            "--workspace".to_string(),
            workspace.root.to_string_lossy().to_string(),
            "--artifact-dir".to_string(),
            workspace.artifacts.to_string_lossy().to_string(),
            "Update smoke file".to_string(),
        ])
        .await;

        std::env::remove_var("AGENT_IDE_MOCK_TOOL");
        std::env::remove_var("AGENT_IDE_MOCK_TOOL_ARGS");
        let exit = result.unwrap();

        assert!(
            !workspace.path("tool-ran.txt").exists(),
            "a command ran without --allow-run: the tool was reachable when it should not exist"
        );
        assert_eq!(exit, ExitCode::ChangesProposed);
    }

    /// 写入工具的端到端证据：模型调用 → 文件真的落盘 → 运行记录里查得到。
    ///
    /// 最后一条是重点。桌面端把工具写入登记成可撤销的 applied diff 卡片，CLI 没有
    /// 审查区，对应物就是 `tool-writes.json`；不写它，磁盘变了而运行记录查不到
    /// 是谁改的。
    #[tokio::test]
    async fn smoke_write_tool_lands_a_file_and_records_it() {
        let _guard = cli_smoke_lock()
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _workspace_guard = workspace::env_test_guard();
        set_mock_llm_env();
        let workspace = SmokeWorkspace::new("writetool", "initial");
        std::env::set_var("AGENT_IDE_MOCK_TOOL", "workspace_write_file");
        std::env::set_var(
            "AGENT_IDE_MOCK_TOOL_ARGS",
            serde_json::json!({
                "path": "written-by-agent.txt",
                "content": "tool wrote this\n",
            })
            .to_string(),
        );

        let result = run_from_args([
            "agent-cli".to_string(),
            "run".to_string(),
            "--workspace".to_string(),
            workspace.root.to_string_lossy().to_string(),
            "--artifact-dir".to_string(),
            workspace.artifacts.to_string_lossy().to_string(),
            "--apply".to_string(),
            "--allow-create".to_string(),
            "--allow-edit".to_string(),
            "--allow-agent-write".to_string(),
            "Update smoke file".to_string(),
        ])
        .await;

        std::env::remove_var("AGENT_IDE_MOCK_TOOL");
        std::env::remove_var("AGENT_IDE_MOCK_TOOL_ARGS");
        result.unwrap();

        assert_eq!(
            fs::read_to_string(workspace.path("written-by-agent.txt")).unwrap(),
            "tool wrote this\n"
        );
        let records: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(workspace.artifacts.join("tool-writes.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(records[0]["file"], "written-by-agent.txt");
        assert_eq!(records[0]["created"], true);
    }

    /// 对照组：同一次调用，只是没给 `--allow-agent-write`。工具不通告也不认领，
    /// 所以文件不该出现 —— 证明它是真的不可达，而不是恰好没被调到。
    #[tokio::test]
    async fn smoke_write_tool_is_absent_without_permission() {
        let _guard = cli_smoke_lock()
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _workspace_guard = workspace::env_test_guard();
        set_mock_llm_env();
        let workspace = SmokeWorkspace::new("nowrite", "initial");
        std::env::set_var("AGENT_IDE_MOCK_TOOL", "workspace_write_file");
        std::env::set_var(
            "AGENT_IDE_MOCK_TOOL_ARGS",
            serde_json::json!({
                "path": "written-by-agent.txt",
                "content": "tool wrote this\n",
            })
            .to_string(),
        );

        let result = run_from_args([
            "agent-cli".to_string(),
            "run".to_string(),
            "--workspace".to_string(),
            workspace.root.to_string_lossy().to_string(),
            "--artifact-dir".to_string(),
            workspace.artifacts.to_string_lossy().to_string(),
            "--apply".to_string(),
            "--allow-create".to_string(),
            "--allow-edit".to_string(),
            "Update smoke file".to_string(),
        ])
        .await;

        std::env::remove_var("AGENT_IDE_MOCK_TOOL");
        std::env::remove_var("AGENT_IDE_MOCK_TOOL_ARGS");
        result.unwrap();

        assert!(
            !workspace.path("written-by-agent.txt").exists(),
            "the write tool was reachable without --allow-agent-write"
        );
        assert!(!workspace.artifacts.join("tool-writes.json").exists());
    }

    /// 预览运行（没有 `--apply`）的承诺是"磁盘一点都不动"，而写入工具会在运行
    /// 途中落盘。所以这个组合必须在跑之前就被拒绝，而不是跑完才发现文件变了。
    #[test]
    fn agent_write_requires_apply() {
        let mut args = RunArgs {
            allow_agent_write: true,
            ..RunArgs::default()
        };

        let error = validate_agent_write_permission(&args).unwrap_err();
        assert_eq!(error.0, ExitCode::InvalidInput);
        assert!(error.1.contains("--apply"), "{}", error.1);

        args.apply = true;
        assert!(validate_agent_write_permission(&args).is_ok());
    }
}
