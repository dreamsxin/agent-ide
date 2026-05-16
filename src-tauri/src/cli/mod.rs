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
}

#[derive(Args, Debug)]
struct DoctorArgs {
    #[arg(long)]
    workspace: Option<PathBuf>,

    #[arg(long, value_enum, default_value = "text")]
    output: OutputMode,
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
}

#[derive(Args, Debug, Clone)]
struct AgentCommandArgs {
    #[command(flatten)]
    run: RunArgs,
    #[arg(value_name = "PROMPT", num_args = 0..)]
    prompt: Vec<String>,
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
}

impl From<ContextModeArg> for ContextCompressionMode {
    fn from(value: ContextModeArg) -> Self {
        match value {
            ContextModeArg::Full => ContextCompressionMode::Full,
            ContextModeArg::Focused => ContextCompressionMode::Focused,
            ContextModeArg::Compact => ContextCompressionMode::Compact,
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum ContextSourceArg {
    GitDiff,
    ProjectTree,
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
    capabilities: Option<CliCapabilities>,
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
            "doctor" | "context" | "plan" | "run" | "help" | "--help" | "-h" | "--version" | "-V"
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
    let artifact_dir = default_artifact_dir(&workspace_path, &run_id);
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
        capabilities: Some(cli_capabilities()),
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
        capabilities: None,
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
    let artifact_dir = args
        .artifact_dir
        .clone()
        .unwrap_or_else(|| default_artifact_dir(&workspace_path, &run_id));
    let prompt = read_prompt(&args, positional_prompt)?;
    validate_repair_permissions(&args)?;
    let llm = build_llm_client(&args)?;
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
            capabilities: None,
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
    )
    .await?;
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
    let mut checks_failed = command_results
        .iter()
        .any(|result| result.exit_code.unwrap_or(-1) != 0);
    let mut repair_chain = Vec::new();

    if args.apply && checks_failed && args.max_iterations > 0 {
        for iteration in 1..=args.max_iterations {
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
            )
            .await?;
            let repair_apply = apply_pending_diffs(&repair_diffs);
            output.event(CliEvent::ApplyFinished {
                applied_count: repair_apply.applied.len(),
                failed_count: repair_apply.failed.len(),
            });
            diffs.extend(repair_diffs.clone());
            apply_results.push(repair_apply.clone());
            command_results = run_cli_checks(&args, &workspace_path, &mut output).await?;
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
            if !repair_apply.failed.is_empty() || !checks_failed {
                break;
            }
        }
    }

    let command_problems = collect_command_problems(&command_results);
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
        repair_chain,
        capabilities: None,
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
    Ok(exit)
}

async fn execute_steps(
    llm: &LlmClient,
    prompt: &str,
    context_text: &str,
    workspace_path: &Path,
    steps: &[TaskStep],
    output: &mut CliOutput,
    cancel_flag: Arc<AtomicBool>,
) -> Result<Vec<FileDiff>, (ExitCode, String)> {
    let output_mode = output.mode;
    let deferred_events = RefCell::new(Vec::<CliEvent>::new());
    let results = agent_runtime::execute_agent_steps(
        llm,
        prompt,
        context_text,
        workspace_path,
        steps,
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
    )
    .await
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
    if !summary.problems.is_empty() {
        write_json(artifact_dir.join("problems.json"), &summary.problems)?;
    }
    if !summary.repair_chain.is_empty() {
        write_json(
            artifact_dir.join("repair-chain.json"),
            &summary.repair_chain,
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
        capabilities: None,
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
        ],
        output_modes: vec!["text".to_string(), "json".to_string(), "ndjson".to_string()],
        context_modes: vec![
            "full".to_string(),
            "focused".to_string(),
            "compact".to_string(),
        ],
        artifacts: vec![
            "summary.json".to_string(),
            "events.json".to_string(),
            "events.ndjson".to_string(),
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
        supports_interactive_review: false,
        supports_git_mutation: false,
        scope: "headless automation runner; not a full command-line IDE".to_string(),
    }
}

async fn run_cli_checks(
    args: &RunArgs,
    workspace_path: &Path,
    output: &mut CliOutput,
) -> Result<Vec<RunProjectTaskResult>, (ExitCode, String)> {
    let mut results = Vec::new();
    for command in &args.run_commands {
        let result =
            project_tasks::run_project_command(command.clone(), workspace_path.to_path_buf())
                .await
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

fn collect_command_problems(command_results: &[RunProjectTaskResult]) -> Vec<ProblemEntry> {
    command_results
        .iter()
        .flat_map(|result| result.problems.clone())
        .collect()
}

fn failed_command_results(command_results: &[RunProjectTaskResult]) -> Vec<RunProjectTaskResult> {
    command_results
        .iter()
        .filter(|result| result.exit_code.unwrap_or(-1) != 0)
        .cloned()
        .collect()
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

fn build_repair_prompt(
    original_prompt: &str,
    iteration: u8,
    command_results: &[RunProjectTaskResult],
    problems: &[ProblemEntry],
) -> String {
    let mut lines = vec![
        format!(
            "Repair iteration {} for the original Agent IDE CLI task.",
            iteration
        ),
        "Original task:".to_string(),
        original_prompt.to_string(),
        String::new(),
        "Checks failed after applying the generated changes. Fix only the failures below."
            .to_string(),
        "Return reviewable Agent IDE diffs only.".to_string(),
        String::new(),
        "Parsed Problems:".to_string(),
    ];

    if problems.is_empty() {
        lines.push("(none parsed)".to_string());
    } else {
        for problem in problems.iter().take(40) {
            lines.push(format!(
                "- {}:{}:{} [{}] {}: {}",
                problem.file,
                problem.line,
                problem.column,
                problem.severity,
                problem.source,
                problem.message
            ));
        }
        if problems.len() > 40 {
            lines.push(format!(
                "... {} more problem(s) omitted",
                problems.len() - 40
            ));
        }
    }

    lines.push(String::new());
    lines.push("Failed command output:".to_string());
    for result in command_results
        .iter()
        .filter(|result| result.exit_code.unwrap_or(-1) != 0)
    {
        let output = [result.stdout.as_str(), result.stderr.as_str()]
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        lines.push(format!(
            "\n$ {} (exit {})\n```text\n{}\n```",
            result.command,
            result
                .exit_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            truncate_for_prompt(&output, 16_000)
        ));
    }

    lines.join("\n")
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    if value.len() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("\n... truncated ...");
    truncated
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
        model,
        provider: "custom".to_string(),
        max_output_tokens: None,
    }))
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

fn is_command_allowed(command: &str, allow_run: &[String]) -> bool {
    let command = normalize_command_pattern(command);
    allow_run.iter().any(|pattern| {
        let pattern = normalize_command_pattern(pattern);
        if pattern == "*" {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            return command.starts_with(prefix.trim_end());
        }
        command == pattern
    })
}

fn normalize_command_pattern(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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
    ContextSourceOptions {
        include_project_tree,
        include_git_diff,
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
        Uuid::new_v4().simple().to_string()[..8].to_string()
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
mod tests {
    use super::*;

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
            "--apply",
            "fix tests",
        ])
        .unwrap();
        match cli.command {
            Some(CliCommand::Run(args)) => {
                assert_eq!(args.run.run_commands, vec!["npm test", "cargo test"]);
                assert_eq!(args.run.allow_run, vec!["npm test", "cargo *"]);
                assert_eq!(args.run.max_iterations, 2);
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
        assert!(!capabilities.supports_interactive_review);
        assert!(!capabilities.supports_git_mutation);
        assert!(capabilities.scope.contains("headless automation"));
    }
}
