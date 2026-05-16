use crate::agent::diff_apply::apply_pending_diffs;
use crate::agent::executor;
use crate::agent::planner;
use crate::agent::state_machine::{ApplyDiffsResult, FileDiff, TaskStep};
use crate::services::context::{
    ContextBuildOptions, ContextCompressionMode, ContextEstimateResponse, ContextSourceOptions,
};
use crate::services::llm_client::{LlmClient, LlmConfig};
use crate::services::llm_profiles;
use crate::services::{context::AgentContext, workspace};
use chrono::Utc;
use clap::error::ErrorKind;
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
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
    errors: Vec<String>,
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

    let diffs = execute_steps(
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

    let exit = if let Some(result) = &apply_result {
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
        ExitCode::ApplyFailed => CliStatus::ApplyFailed,
        _ => CliStatus::Ok,
    };

    output.event(CliEvent::RunFinished {
        status: status.clone(),
        exit_code: exit.as_u8(),
    });

    let errors = apply_result
        .as_ref()
        .map(|result| {
            result
                .failed
                .iter()
                .map(|failure| format!("{}: {}", failure.file, failure.message))
                .collect()
        })
        .unwrap_or_default();

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
    let mut all_diffs = Vec::new();

    for (index, step) in steps.iter().enumerate() {
        output.text(format!(
            "--- Step {}/{}: {} ---",
            index + 1,
            steps.len(),
            step.title
        ));
        output.event(CliEvent::StepStarted {
            step_id: step.id.clone(),
            title: step.title.clone(),
        });

        let step_context = build_step_context(prompt, step, context_text, workspace_path);
        let tx = output.token_sender();
        let response =
            executor::execute_step(llm, &step.title, &step_context, cancel_flag.clone(), tx)
                .await
                .map_err(|err| (ExitCode::ProviderFailed, err))?;
        output.text("");
        let diffs = executor::parse_diffs(&response);
        output.event(CliEvent::StepFinished {
            step_id: step.id.clone(),
            response_chars: response.chars().count(),
            diff_count: diffs.len(),
        });
        all_diffs.extend(diffs);
    }

    Ok(all_diffs)
}

fn build_step_context(
    prompt: &str,
    step: &TaskStep,
    context_text: &str,
    workspace_path: &Path,
) -> String {
    let mut step_ctx = format!(
        "Task: {}\nStep: {}\nType: {}\nContext:\n{}",
        prompt, step.title, step.step_type, context_text
    );

    let mut paths_to_try: Vec<PathBuf> = Vec::new();
    for word in step.title.split_whitespace() {
        let w = word.trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '.' && c != '/' && c != '\\' && c != '-' && c != '_'
        });
        if w.contains('.') && w.len() > 3 {
            paths_to_try.push(workspace_path.join(w));
            paths_to_try.push(PathBuf::from(w));
        }
    }

    if paths_to_try.is_empty() {
        let exts = ["js", "ts", "jsx", "tsx", "py", "rs", "go", "java"];
        if let Ok(entries) = fs::read_dir(workspace_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if exts.contains(&ext) {
                        paths_to_try.push(path);
                    }
                }
            }
        }
    }

    let mut found_names: Vec<String> = Vec::new();
    for path in &paths_to_try {
        if path.exists() && path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if !found_names.contains(&name.to_string()) {
                    if let Ok(file_content) = fs::read_to_string(path) {
                        step_ctx.push_str(&format!(
                            "\n\n--- File: {} ---\n```\n{}\n```",
                            name, file_content
                        ));
                        found_names.push(name.to_string());
                    }
                }
            }
        }
    }

    if !found_names.is_empty() {
        step_ctx.push_str("\n\n(File contents above are current - base your diff on them)");
    }

    step_ctx
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
        errors: vec![error],
    }
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
}
