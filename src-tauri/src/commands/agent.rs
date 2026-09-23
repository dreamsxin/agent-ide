use crate::agent::executor;
use crate::agent::multi_agent::{default_pipeline, AgentRole, PipelineStage};
use crate::agent::orchestrator::AgentOrchestrator;
use crate::agent::state_machine::{
    AgentMode, AgentState, ApplyDiffsResult, FileDiff, IdeMode, SddArtifact, TaskStep,
};
use crate::services::agent_runtime;
use crate::services::context::{
    AgentContext, ContextBudget, ContextBuildOptions, ContextCompressionMode,
    ContextEstimateResponse, ContextSourceOptions,
};
use crate::services::llm_client::{LlmClient, LlmConfig};
use crate::services::llm_profiles::{
    self, LlmProfileResponse, LlmProfilesConfig, LlmProfilesResponse, SaveLlmProfileRequest,
};
use crate::services::workspace;
use serde::{Deserialize, Serialize};
use std::sync::{atomic::AtomicBool, Arc};
use tauri::Emitter;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;

/// Global Agent state. Uses tokio::sync::Mutex for async orchestration.
pub struct AgentGlobalState {
    pub orchestrator: Arc<Mutex<AgentOrchestrator>>,
    pub llm_profiles: Arc<std::sync::Mutex<LlmProfilesConfig>>,
    pub active_role: Arc<std::sync::Mutex<AgentRole>>,
    pub pipeline_stages: Arc<std::sync::Mutex<Vec<PipelineStage>>>,
    pub context_compression: Arc<std::sync::Mutex<ContextCompressionMode>>,
    /// 当前运行取消开关的句柄。见 `CancelRegistry` —— Stop 必须能在不持有
    /// orchestrator 锁的情况下拉开关。
    pub cancel_registry: crate::agent::orchestrator::CancelRegistry,
    /// 挂起的逐动作批准请求。和 `cancel_registry` 同理由：`resolve_agent_approval`
    /// 和 Stop 都不能排在它们要放行/拒掉的那次工具调用后面。
    pub approval_registry: crate::agent::approval::ApprovalRegistry,
}

impl AgentGlobalState {
    pub fn new() -> Self {
        let profiles_config = llm_profiles::load_or_default_config();
        let context_compression = profiles_config.context_compression.clone();

        let mut orchestrator = AgentOrchestrator::new();
        let cancel_registry = orchestrator.cancel_registry();
        // 把上一次会话留下的、撤不回的动作接回来。这是这些动作唯一的补偿，而在此之前
        // 它只活在内存里 —— 关掉应用，"它做过什么"就再没人知道。这里是唯一合适的位置：
        // 状态在任何命令之前构造，而工作区路径已经在磁盘上。
        orchestrator
            .restore_external_actions(crate::agent::external_log::load_for_current_workspace());

        Self {
            orchestrator: Arc::new(Mutex::new(orchestrator)),
            llm_profiles: Arc::new(std::sync::Mutex::new(profiles_config)),
            active_role: Arc::new(std::sync::Mutex::new(AgentRole::Coder)),
            pipeline_stages: Arc::new(std::sync::Mutex::new(default_pipeline())),
            context_compression: Arc::new(std::sync::Mutex::new(context_compression)),
            cancel_registry,
            approval_registry: crate::agent::approval::ApprovalRegistry::new(),
        }
    }

    /// 这次运行的逐动作批准通道。
    ///
    /// 事件出口是 `AppHandle`：桌面端是唯一有人能应答的入口。headless 入口不装通道，
    /// 于是撤不回的动作在那里一律被拒 —— 那是刻意的默认方向，不是缺失。
    fn approval_gate(&self, app_handle: &AppHandle) -> crate::agent::approval::ApprovalGate {
        crate::agent::approval::ApprovalGate::new(
            self.approval_registry.clone(),
            Arc::new(app_handle.clone()),
        )
    }

    /// Get a cloned LLM client plus a fresh per-run usage meter.
    ///
    /// 每次取客户端都新建一个 meter，等价于"每次运行一个记账周期"。上限在
    /// `send_chat_request` 里强制，所以只要客户端是从这里拿的，就一定被记账、
    /// 也一定受上限约束。已知取舍：`continue_agent_pipeline` 恢复暂停的运行时
    /// 会重新开始记账，续跑的部分不计入上一段的额度。
    /// 取一个客户端，可选地只把**模型名**换掉。
    ///
    /// `model_override` 的用途是"这一次换个模型试试"，而不用为此存一个 profile。它只改模型名：
    /// endpoint、key、预算、价格、上限统统还是那个 profile 的 —— 换模型不该顺带换掉
    /// 用户配的花钱上限。价格也因此可能对不上（profile 里的单价是给它原来那个模型配的），
    /// 所以这条覆盖必须在界面上说出来，并且写一条 action log。
    ///
    /// 空串当成没设置：前端的输入框清空之后送过来的就是空串，而一个空模型名会变成供应商那边
    /// 一句看不懂的 400。
    pub fn get_llm_client(
        &self,
        profile_id: Option<&str>,
        model_override: Option<&str>,
    ) -> Result<(LlmClient, Arc<crate::services::llm_client::RunUsageMeter>), String> {
        let mut config = self.get_llm_config(profile_id)?;
        if let Some(model) = model_override.map(str::trim).filter(|m| !m.is_empty()) {
            // 只改模型名。请求里真正依赖它的是 `output_token_key`（o1/o3/o4/gpt-5 用
            // `max_completion_tokens`），那个函数读的就是 `config.model`。
            config.model = model.to_string();
        }
        // 配置了本地模型的 profile 在这里就被挡住：进程内推理已经移除，静默降级成
        // 远端调用只会让用户收到一串莫名其妙的 401/404。这是这条路径上唯一的检查点。
        if let Some(local) = &config.local_model_config {
            return Err(crate::services::llm_client::local_inference_removed(local));
        }
        let (pricing, max_spend_micros) = self.get_run_spend_cap(profile_id);
        let meter = Arc::new(
            crate::services::llm_client::RunUsageMeter::new(self.get_run_token_cap(profile_id))
                .with_spend_cap(pricing, max_spend_micros),
        );
        Ok((
            LlmClient::new(config).with_usage_meter(meter.clone()),
            meter,
        ))
    }

    pub fn get_run_token_cap(&self, profile_id: Option<&str>) -> Option<u64> {
        let profiles = self.llm_profiles.lock().ok()?;
        llm_profiles::run_token_cap(&profiles, profile_id)
    }

    /// 当前 profile 的价格与单次运行金额上限。锁拿不到时退回"没配置"：
    /// 这只会让上限不执行，而 unwrap 会让整次运行崩掉。
    pub fn get_run_spend_cap(
        &self,
        profile_id: Option<&str>,
    ) -> (
        Option<crate::services::llm_client::TokenPricing>,
        Option<u64>,
    ) {
        let Ok(profiles) = self.llm_profiles.lock() else {
            return (None, None);
        };
        llm_profiles::run_spend_cap(&profiles, profile_id)
    }

    pub fn get_llm_config(&self, profile_id: Option<&str>) -> Result<LlmConfig, String> {
        let profiles = self.llm_profiles.lock().map_err(|e| e.to_string())?;
        llm_profiles::resolve_llm_config(&profiles, profile_id)
    }

    pub fn get_context_budget(&self, profile_id: Option<&str>) -> Option<ContextBudget> {
        let profiles = self.llm_profiles.lock().ok()?;
        llm_profiles::context_budget(&profiles, profile_id)
    }
}

/// Agent status response DTO.
#[derive(Debug, Serialize)]
pub struct AgentStatus {
    pub state: String,
    pub mode: String,
    #[serde(rename = "ideMode")]
    pub ide_mode: String,
    #[serde(rename = "currentRunId")]
    pub current_run_id: Option<String>,
    #[serde(rename = "lastRunId")]
    pub last_run_id: Option<String>,
}

/// Send-prompt request DTO.
#[derive(Debug, Deserialize)]
pub struct SendPromptRequest {
    pub prompt: String,
    #[serde(rename = "contextFiles")]
    pub context_files: Vec<String>,
    #[serde(rename = "activeFile")]
    pub active_file: Option<String>,
    #[serde(rename = "activeFileContent")]
    pub active_file_content: Option<String>,
    pub selection: Option<String>,
    #[serde(rename = "profileId")]
    pub profile_id: Option<String>,
    /// 只换模型名，不换 profile。见 `get_llm_client`：key、endpoint、预算、价格、上限都还是
    /// 那个 profile 的，所以价格可能对不上 —— 这条覆盖会写进 action log。
    #[serde(default, rename = "modelOverride")]
    pub model_override: Option<String>,
    #[serde(rename = "contextCompression")]
    pub context_compression: Option<String>,
    #[serde(default, rename = "contextSources")]
    pub context_sources: Option<ContextSourceOptions>,
    /// MCP 工具放行策略：`deny` / `auto_approved_only` / `allow_all`。
    /// 缺省按 `auto_approved_only` 处理。
    #[serde(default, rename = "toolApproval")]
    pub tool_approval: Option<String>,
    /// Auto 模式自动应用时是否允许创建新文件。缺省 false：
    /// 未显式授权时，新建文件的 diff 留给人工审查而不是静默写盘。
    #[serde(default, rename = "allowFileCreate")]
    pub allow_file_create: bool,
    /// 是否允许 Agent 自己跑项目声明的检查命令。缺省 false。
    /// 授权后暴露的仍然只有项目自己声明的、非长驻的命令，不是任意 shell。
    #[serde(default, rename = "allowCommandRun")]
    pub allow_command_run: bool,
    #[serde(rename = "runId")]
    pub run_id: Option<String>,
    #[serde(rename = "ideMode")]
    pub ide_mode: Option<String>,
    /// IDE 当下的运行状况（问题、终端、失败的检查、日志）。
    ///
    /// 单独一个字段而不是拼在 `prompt` 里：拼进提示词的话，估算面板和预算裁剪都看不见
    /// 它，而它能有上万字符。作为上下文段落进来才会被计量。
    #[serde(default, rename = "ideRuntime")]
    pub ide_runtime: Option<String>,
    /// 是否允许驱动浏览器。和写盘分开：打开页面不改工作区，但会把内容送到某个站点。
    #[serde(default, rename = "allowBrowserUse")]
    pub allow_browser_use: bool,
    /// 允许访问的 origin 清单；空清单等于不许，`allowBrowserUse` 也救不了。
    #[serde(default, rename = "browserOrigins")]
    pub browser_origins: Option<Vec<String>>,
    /// 是否允许观察桌面（窗口枚举，只读）。和浏览器分开：范围一个是站点、一个是应用。
    #[serde(default, rename = "allowComputerUse")]
    pub allow_computer_use: bool,
    /// 允许被观察的应用清单；空清单等于不许，开关也救不了。
    #[serde(default, rename = "computerApps")]
    pub computer_apps: Option<Vec<String>>,
    /// 是否允许截窗口内容。和观察分开：标题是"Signal 开着"，截图是消息本身。
    #[serde(default, rename = "allowComputerCapture")]
    pub allow_computer_capture: bool,
    /// 允许被截图的应用清单；空清单等于不许。
    #[serde(default, rename = "captureApps")]
    pub capture_apps: Option<Vec<String>>,
    /// 是否允许读一个已经打开的页面的正文。和 `allowBrowserUse` 分开：列表说"你开着
    /// 这个站点"，正文是站点上的内容，包括只有登录之后才看得到的那部分。
    #[serde(default, rename = "allowPageRead")]
    pub allow_page_read: bool,
    /// 允许被读取正文的 origin 清单；空清单等于不许。
    #[serde(default, rename = "pageReadOrigins")]
    pub page_read_origins: Option<Vec<String>>,
    /// 是否允许往窗口里注入点击。这个产品里最狠的一档：撤不回，而且能点掉确认框。
    #[serde(default, rename = "allowComputerInput")]
    pub allow_computer_input: bool,
    /// 允许被点击的应用清单；空清单等于不许。
    #[serde(default, rename = "inputApps")]
    pub input_apps: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct SaveSddArtifactRequest {
    pub artifact: SddArtifact,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Serialize)]
pub struct SavedSddArtifactResponse {
    pub path: String,
    pub artifact: SddArtifact,
}

#[derive(Debug, Deserialize)]
pub struct EstimateContextRequest {
    #[serde(rename = "contextFiles")]
    pub context_files: Vec<String>,
    #[serde(rename = "activeFile")]
    pub active_file: Option<String>,
    #[serde(rename = "activeFileContent")]
    pub active_file_content: Option<String>,
    pub selection: Option<String>,
    #[serde(rename = "profileId")]
    pub profile_id: Option<String>,
    // 这里**没有** `modelOverride`：估算只用得到 profile 的 `max_context_tokens`，而覆盖
    // 不带窗口（ROADMAP 145 里"没做"的那条）。留一个读不到的字段，就是前端照发、后端照忽略
    // 的死参数 —— clippy 的 dead_code 抓到的正是它。
    #[serde(rename = "contextCompression")]
    pub context_compression: Option<String>,
    #[serde(default, rename = "contextSources")]
    pub context_sources: Option<ContextSourceOptions>,
    /// 和 `SendPromptRequest.ide_runtime` 同一段内容：估算必须和真正发出去的一致，
    /// 否则面板上的数字会系统性偏小。
    #[serde(default, rename = "ideRuntime")]
    pub ide_runtime: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RunAgentStepRequest {
    pub step: TaskStep,
    #[serde(rename = "contextFiles")]
    pub context_files: Vec<String>,
    #[serde(rename = "activeFile")]
    pub active_file: Option<String>,
    #[serde(rename = "activeFileContent")]
    pub active_file_content: Option<String>,
    pub selection: Option<String>,
    #[serde(rename = "profileId")]
    pub profile_id: Option<String>,
    /// 只换模型名，不换 profile。见 `get_llm_client`：key、endpoint、预算、价格、上限都还是
    /// 那个 profile 的，所以价格可能对不上 —— 这条覆盖会写进 action log。
    #[serde(default, rename = "modelOverride")]
    pub model_override: Option<String>,
    #[serde(rename = "contextCompression")]
    pub context_compression: Option<String>,
    #[serde(default, rename = "contextSources")]
    pub context_sources: Option<ContextSourceOptions>,
    /// 同 `SendPromptRequest::tool_approval`
    #[serde(default, rename = "toolApproval")]
    pub tool_approval: Option<String>,
    /// 同 `SendPromptRequest::allow_command_run`
    #[serde(default, rename = "allowCommandRun")]
    pub allow_command_run: bool,
    /// 同 `SendPromptRequest::allow_file_create`
    #[serde(default, rename = "allowFileCreate")]
    pub allow_file_create: bool,
    /// 同 `SendPromptRequest::allow_browser_use`
    #[serde(default, rename = "allowBrowserUse")]
    pub allow_browser_use: bool,
    /// 同 `SendPromptRequest::browser_origins`
    #[serde(default, rename = "browserOrigins")]
    pub browser_origins: Option<Vec<String>>,
    /// 同 `SendPromptRequest::allow_computer_use`
    #[serde(default, rename = "allowComputerUse")]
    pub allow_computer_use: bool,
    /// 同 `SendPromptRequest::computer_apps`
    #[serde(default, rename = "computerApps")]
    pub computer_apps: Option<Vec<String>>,
    /// 同 `SendPromptRequest::allow_computer_capture`
    #[serde(default, rename = "allowComputerCapture")]
    pub allow_computer_capture: bool,
    /// 同 `SendPromptRequest::capture_apps`
    #[serde(default, rename = "captureApps")]
    pub capture_apps: Option<Vec<String>>,
    /// 同 `SendPromptRequest::allow_page_read`
    #[serde(default, rename = "allowPageRead")]
    pub allow_page_read: bool,
    /// 同 `SendPromptRequest::page_read_origins`
    #[serde(default, rename = "pageReadOrigins")]
    pub page_read_origins: Option<Vec<String>>,
    /// 同 `SendPromptRequest::allow_computer_input`
    #[serde(default, rename = "allowComputerInput")]
    pub allow_computer_input: bool,
    /// 同 `SendPromptRequest::input_apps`
    #[serde(default, rename = "inputApps")]
    pub input_apps: Option<Vec<String>>,
    #[serde(rename = "extraPrompt")]
    pub extra_prompt: Option<String>,
    #[serde(rename = "regeneratedFromDiffId")]
    pub regenerated_from_diff_id: Option<String>,
    #[serde(rename = "regeneratedFromHunkIndex")]
    pub regenerated_from_hunk_index: Option<usize>,
    #[serde(rename = "runId")]
    pub run_id: Option<String>,
}

/// Get the current Agent state.
#[tauri::command]
pub async fn get_agent_state(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<AgentStatus, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(AgentStatus {
        state: orch.state_mgr.state.to_string(),
        mode: orch.mode.to_string(),
        ide_mode: orch.ide_mode.to_string(),
        current_run_id: orch.current_run_id.clone(),
        last_run_id: orch.last_run_id.clone(),
    })
}

#[tauri::command]
pub async fn estimate_agent_context(
    request: EstimateContextRequest,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<ContextEstimateResponse, String> {
    let context_budget = agent_state.get_context_budget(request.profile_id.as_deref());
    // 估算也要带上历史：面板上的数字是用来决定"要不要清一下上下文"的，而真正发出去的
    // prompt 里有这几轮对话。少算这一段，越聊越久数字就越乐观，用户看着还有余量却已经
    // 在截断了。
    let conversation = {
        let orch = agent_state.orchestrator.lock().await;
        orch.conversation_digest()
    };
    let mut context = build_agent_context(
        request.active_file,
        request.active_file_content,
        request.selection,
        request.context_files,
        request.ide_runtime,
        conversation,
    );
    let context_sources = request
        .context_sources
        .unwrap_or_else(default_context_sources);
    context.enrich_from_workspace_with_sources(&context_sources);
    let compression = resolve_context_compression(
        &agent_state.context_compression,
        request.context_compression.as_deref(),
    )?;

    Ok(context.estimate_prompt_context(&ContextBuildOptions::new(compression, context_budget)))
}

/// Send a prompt to the Agent.
#[tauri::command]
pub async fn send_agent_prompt(
    request: SendPromptRequest,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
    mcp_state: State<'_, crate::commands::mcp::McpState>,
) -> Result<String, String> {
    let (llm, usage_meter) = agent_state.get_llm_client(
        request.profile_id.as_deref(),
        request.model_override.as_deref(),
    )?;
    let tool_policy =
        crate::services::mcp::McpToolPolicy::from_request(request.tool_approval.as_deref());
    // 这次运行的副作用开关先造出来，**交给授权**，之后所有需要它的地方都从授权里取：
    // MCP 执行器、内置工具面、`claim_run_for`（进 `RunLease` 和 `CancelRegistry`）。
    // 一个来源，所以"三处拿到的是同一个开关"是结构性的，而不是靠三行抄对。
    let side_effect_switch = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // 内置工作区工具：让模型自己决定读哪些文件，而不是只能吃预打包的上下文。
    // 命令执行和写入按本次运行的权限决定是否暴露。
    //
    // 写权限只跟 Auto 模式挂钩，且在这里就取好快照：Auto 本来就会在流水线结束后
    // 自动落盘，运行途中写不构成新的特权等级；Suggest 的约定是"人先看再落盘"。
    let allow_write = {
        let orch = agent_state.orchestrator.lock().await;
        matches!(orch.mode, AgentMode::Auto)
    };
    let mut tool_permissions = agent_tool_permissions(
        request.allow_command_run,
        allow_write,
        request.allow_file_create,
        ExternalGrants {
            allow_browser: request.allow_browser_use,
            browser_origins: request.browser_origins.clone().unwrap_or_default(),
            allow_page_read: request.allow_page_read,
            page_read_origins: request.page_read_origins.clone().unwrap_or_default(),
            allow_computer: request.allow_computer_use,
            computer_apps: request.computer_apps.clone().unwrap_or_default(),
            allow_capture: request.allow_computer_capture,
            capture_apps: request.capture_apps.clone().unwrap_or_default(),
            allow_input: request.allow_computer_input,
            input_apps: request.input_apps.clone().unwrap_or_default(),
        },
        agent_state.approval_gate(&app_handle),
    );
    // 移进去而不是克隆：局部变量之后就不能再交给别人，多一个消费者会编译不过
    tool_permissions.adopt_cancel(side_effect_switch);
    // 派子 Agent 的通道。给的是**接 MCP 之前**的客户端：子 Agent 的工具表在委派时按它自己
    // 的只读权限重建（`SubagentChannel::child_client`），父运行的 MCP 工具不该出现在里面。
    // 发不出工具表的档位拿不到通道，于是 `delegate_task` 根本不会被通告出去。
    let mut tool_permissions = tool_permissions.with_subagent(
        crate::agent::workspace_tools::SubagentChannel::for_run(&llm),
    );
    let (llm, tool_invoker) = crate::commands::mcp::attach_mcp_tools(
        &mcp_state.registry,
        std::sync::Arc::new(app_handle.clone()),
        llm,
        tool_policy,
        &tool_permissions,
    )
    .await;
    let (llm, tool_invoker) = crate::agent::workspace_tools::attach_workspace_tools(
        llm,
        tool_invoker,
        Some(workspace_tool_logger(&app_handle)),
        tool_permissions.clone(),
    );
    let context_budget = agent_state.get_context_budget(request.profile_id.as_deref());
    let context_sources = request
        .context_sources
        .unwrap_or_else(default_context_sources);
    let compression = resolve_context_compression(
        &agent_state.context_compression,
        request.context_compression.as_deref(),
    )?;

    let pipeline = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let ide_mode = request
        .ide_mode
        .as_deref()
        .map(IdeMode::from_str)
        .transpose()?
        .unwrap_or(IdeMode::Code);
    // 只在准备阶段持锁。运行本身由 `drive_run` 自己按阶段短持锁 —— 整段持锁会让
    // stop / apply / 状态查询全部排在模型调用后面。
    let (lease, conversation) = {
        let mut orch = agent_state.orchestrator.lock().await;
        // 抢执行权要在改任何字段之前：抢不到就说明已经有运行在跑，这时候
        // 覆写它的工具面或记账器会把那次运行改坏。开关从授权里取，见 `claim_run_for`
        let lease = claim_run_for(&mut orch, request.run_id.clone(), &mut tool_permissions)?;
        orch.tool_invoker = tool_invoker;
        orch.tool_policy = tool_policy;
        orch.tool_permissions = tool_permissions.clone();
        orch.allow_file_create = request.allow_file_create;
        orch.start_usage_accounting(usage_meter.clone());
        // 历史必须和抢执行权在**同一个**临界区里取。`truncate_agent_conversation` /
        // `start_new_agent_session` / `resume_agent_session` / `delete_agent_session` 都不需要
        // 执行权就能改历史，所以只要这两件事分开，用户点了"切掉这一轮"之后启动的这次运行
        // 仍然会把那几轮发出去 —— 界面上已经没有了，模型还在看着。
        (lease, orch.conversation_digest())
    };
    let claim = lease.claim;

    // 上下文在抢到执行权之后再装：`enrich_from_workspace_with_sources` 要读项目树、
    // 跑一次 git diff，抢不到执行权时那些都是白干的。
    let mut context = build_agent_context(
        request.active_file,
        request.active_file_content,
        request.selection,
        request.context_files,
        request.ide_runtime,
        // 把之前几轮喂回去：没有这一步每次 prompt 都是冷启动
        conversation,
    );
    context.enrich_from_workspace_with_sources(&context_sources);
    emit_project_memory_warning(&agent_state.orchestrator, &app_handle, &context).await;
    emit_model_override_log(
        &agent_state,
        &app_handle,
        request.profile_id.as_deref(),
        request.model_override.as_deref(),
    )
    .await;

    let prompt_for_history = request.prompt.clone();
    let outcome = crate::agent::orchestrator::drive_run(
        &agent_state.orchestrator,
        request.prompt,
        context,
        compression,
        context_budget,
        context_sources,
        pipeline,
        ide_mode,
        lease.cancel,
        &llm,
        std::sync::Arc::new(app_handle.clone()),
    )
    .await;

    let mut orch = agent_state.orchestrator.lock().await;
    match outcome {
        Ok(()) => {
            finish_agent_run(
                &mut orch,
                &app_handle,
                &tool_permissions,
                &usage_meter,
                &llm,
                claim,
            );
            orch.record_conversation_turn(&prompt_for_history);
        }
        Err(err) if is_cancelled_error(&err) => {
            finish_agent_run(
                &mut orch,
                &app_handle,
                &tool_permissions,
                &usage_meter,
                &llm,
                claim,
            );
            orch.state_mgr.set(AgentState::Idle);
            let _ = app_handle.emit("agent-state-changed", orch.state_payload());
            return Ok("Agent task cancelled".to_string());
        }
        Err(err) => {
            finish_agent_run(
                &mut orch,
                &app_handle,
                &tool_permissions,
                &usage_meter,
                &llm,
                claim,
            );
            return Err(err);
        }
    }

    Ok("Agent task completed".to_string())
}

/// 内置工作区工具的 action log 回调。
///
/// 发事件的责任留在命令层：agent 层直接持有 `AppHandle` 会把 Tauri runtime
/// 拖进 lib 测试二进制，整个测试套件会在加载阶段就起不来。
fn workspace_tool_logger(app_handle: &AppHandle) -> crate::agent::workspace_tools::ToolCallLogger {
    let app = app_handle.clone();
    std::sync::Arc::new(move |level: &str, summary: &str, details: &str| {
        let entry = crate::agent::orchestrator::ActionLogEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: level.to_string(),
            phase: "workspace_tool_call".to_string(),
            role: None,
            stage: Some("Tool Call".to_string()),
            summary: summary.to_string(),
            details: details.to_string(),
            context_summary: None,
            diff_summary: None,
        };
        let _ = app.emit("agent-action-log", entry);
    })
}

/// 从项目**自己声明的**任务里挑出允许 Agent 执行的命令。
///
/// 纯函数，任务清单从参数进来 —— 扫盘那步（`discover_project_tasks`）留在调用方，
/// 这样这条授权规则本身可以直接测。两条规则：
///   * 没授权就一条都不给，哪怕项目声明了一堆任务；
///   * 滤掉长驻命令。验证是"跑完看结果"，`npm run dev` 永远不退出，
///     放进清单等于给 Agent 一个能把运行挂死的工具。
fn allowed_agent_commands(
    tasks: Vec<crate::services::project_tasks::ProjectTask>,
    allow_command_run: bool,
) -> Vec<String> {
    if !allow_command_run {
        return Vec::new();
    }
    tasks
        .into_iter()
        .map(|task| task.command)
        .filter(|command| !crate::services::verification::is_long_running_command(command))
        .collect()
}

/// 一次运行的**外部**授权，四对开关 + 清单。
///
/// 收成一个具名字段的结构，而不是继续往 `agent_tool_permissions` 后面加位置参数：加到
/// 第十个参数时调用点已经是一串 `false, Vec::new(), false, Vec::new()`，"这个 true 是哪档
/// 权限"只能靠数位置 —— 而数错一位的后果是把截图授权当成了浏览器授权。四档彼此独立，
/// 每一档都是"开关 + 非空清单"两个条件同时成立才放行。
#[derive(Debug, Default, Clone)]
pub struct ExternalGrants {
    pub allow_browser: bool,
    pub browser_origins: Vec<String>,
    pub allow_page_read: bool,
    pub page_read_origins: Vec<String>,
    pub allow_computer: bool,
    pub computer_apps: Vec<String>,
    pub allow_capture: bool,
    pub capture_apps: Vec<String>,
    pub allow_input: bool,
    pub input_apps: Vec<String>,
}

/// 本次运行允许 Agent 执行哪些命令。
///
/// 清单由后端从**项目自己声明的**任务推导（package.json scripts、Cargo），不是
/// 模型自选、也不需要用户手写通配符。未授权时返回空清单，命令工具连通告都不会出现。
fn agent_tool_permissions(
    allow_command_run: bool,
    allow_write: bool,
    allow_create: bool,
    grants: ExternalGrants,
    approval: crate::agent::approval::ApprovalGate,
) -> crate::agent::workspace_tools::WorkspaceToolPermissions {
    // 未授权时连扫都不扫：`discover_project_tasks` 要读 package.json / Cargo.toml。
    // 下面 `allowed_agent_commands` 里那次判断不是重复 —— 那条是授权规则本身，
    // 是被测试钉住的东西；这条只是省掉一次没人会用到的磁盘读。
    let tasks = if allow_command_run {
        crate::services::project_tasks::discover_project_tasks(None).unwrap_or_default()
    } else {
        Vec::new()
    };
    crate::agent::workspace_tools::WorkspaceToolPermissions::new(
        allowed_agent_commands(tasks, allow_command_run),
        allow_write,
        allow_create,
    )
    .with_browser(grants.allow_browser, grants.browser_origins)
    .with_page_read(grants.allow_page_read, grants.page_read_origins)
    .with_computer(grants.allow_computer, grants.computer_apps)
    .with_capture(grants.allow_capture, grants.capture_apps)
    .with_input(grants.allow_input, grants.input_apps)
    // 批准通道是必填参数而不是可选的 `.with_approval()` 调用：漏掉它的运行会把每一次
    // 撤不回的动作都拒掉（`Unattended`），而那种"功能整体消失"的故障恰恰是本项目
    // 反复出现的一类 —— 加了个新东西却没接上它的消费者。让编译器管这件事。
    .with_approval(approval)
}

/// 用这次授权自己带着的副作用开关去抢执行权，并把运行 id 记进授权。
///
/// 开关只有一个来源：命令层铸造它、`adopt_cancel` 交给授权，`try_begin_run` 再从
/// **这一份**取出来。以前这是两行相邻代码、四条命令各抄一遍：一次是给授权、一次是给
/// 租约，两次传的都可以是不同的 `Arc` 而编译器不会说话 —— 传错的后果是 Stop 之后工具
/// 照跑。这里只读一次，于是"两边是同一个开关"由结构保证，而不是靠抄对。
///
/// run_id 也在这里写：撤不回的动作要认领**这一次**运行，而 id 只有拿到执行权之后才确定。
fn claim_run_for(
    orch: &mut crate::agent::orchestrator::AgentOrchestrator,
    run_id: Option<String>,
    permissions: &mut crate::agent::workspace_tools::WorkspaceToolPermissions,
) -> Result<crate::agent::orchestrator::RunLease, String> {
    let lease = orch.try_begin_run(run_id, permissions)?;
    permissions.run_id = orch.current_run_id.clone();
    Ok(lease)
}

/// 把撤不回的外部动作登记到 orchestrator，并写进操作日志。
///
/// 浏览器动作没有 `previous` 可以还原，所以记录**就是**我们唯一能兑现的承诺：哪个
/// 站点、什么时候、成功还是被拒。被拒的调用也记 —— "模型试图打开一个没授权的站点"
/// 只在返回值里说一句，会随着这一轮对话一起消失。
///
/// 登记和发日志都要做：日志是当场能看见的那一份，orchestrator 上那份是刷新前端、
/// 重新读回时还在的那一份。只发日志的版本在审计里被指出来过 —— 窗口没在听，记录
/// 就随着 `take_external_actions` 的排空一起没了。
fn publish_external_actions(
    orch: &mut crate::agent::orchestrator::AgentOrchestrator,
    events: &dyn crate::agent::events::RunEvents,
    permissions: &crate::agent::workspace_tools::WorkspaceToolPermissions,
) {
    let actions = permissions.take_external_actions();
    if actions.is_empty() {
        return;
    }
    let recorded = orch.record_external_actions(actions, permissions.run_id.clone());
    // 落盘。内存里那份会跟着进程一起消失，而这些动作撤不回 —— 一份只活到关窗为止的
    // 审计记录，在用户真正需要它的那一天（"昨天它到底开了什么页面"）正好是空的。
    //
    // 落盘失败、或者旧日志读不出来被挪走，都要作为警告说出来：静默失败在这里等于把
    // 补偿控制关掉而没人知道，而"Agent 什么都没做"和"记录写不进去"的下一步完全不同。
    let persisted = crate::agent::external_log::append_for_current_workspace(&recorded);
    if let Some(warning) = persisted.warning() {
        orch.emit_run_action_log(
            events,
            "warn",
            "external_action",
            "The external action log could not be updated",
            &warning,
        );
    }
    // `_cancelled` 也算没发生：漏掉它的话，一次被 Stop 拦下的导航会被算进
    // "Agent performed N browser action(s) … cannot be undone" —— 在这个产品唯一
    // 承诺可信的地方说一件没发生的事，比记漏还糟。
    let refused = recorded
        .iter()
        .filter(|action| {
            action.kind.ends_with("_refused")
                || action.kind.ends_with("_failed")
                || action.kind.ends_with("_cancelled")
        })
        .count();
    let performed = recorded.len() - refused;
    let details = recorded
        .iter()
        .map(|action| format!("{}: {} — {}", action.kind, action.target, action.detail))
        .collect::<Vec<_>>()
        .join("\n");
    orch.emit_run_action_log(
        events,
        if refused > 0 { "warn" } else { "info" },
        "external_action",
        &format!(
            "Agent performed {} external action(s){}",
            performed,
            if refused > 0 {
                format!(", {} refused, failed or stopped", refused)
            } else {
                String::new()
            }
        ),
        &format!("{}\nThese cannot be undone.", details),
    );
}

/// 把 Agent 写入工具落下的改动登记进审查区并通知前端。
///
/// 不做这一步的话，直接写盘等于绕过审查：磁盘变了而 Diff 视图空着，用户看不到
/// Agent 改了什么，也没有撤销入口。
fn publish_tool_writes(
    orch: &mut crate::agent::orchestrator::AgentOrchestrator,
    events: &dyn crate::agent::events::RunEvents,
    permissions: &crate::agent::workspace_tools::WorkspaceToolPermissions,
) {
    let writes = permissions.take_writes();
    // 事后比对发现的那些（目前只有 MCP）和工具自报的一起发布：它们同样有写前内容，
    // 所以同样能进审查区、同样能撤销 —— 区别只在卡片上那句"怎么知道的"。
    let detected = permissions.take_detected_writes();
    if writes.is_empty() && detected.is_empty() {
        return;
    }
    // 运行 id 从这批授权自己带的那个取，不读 orchestrator 的 `current_run_id`：被 Stop 的
    // 运行可能在下一个 prompt 开跑之后才排空写入，那时读到的是后一次运行的 id ——
    // "撤销这一轮"就会去还原另一轮写的文件。和 `record_external_actions` 同一条规矩。
    let mut recorded = orch.record_tool_writes(writes, permissions.run_id.clone());
    for (tool, writes) in detected {
        recorded.extend(orch.record_detected_writes(&tool, writes, permissions.run_id.clone()));
    }
    if recorded.is_empty() {
        return;
    }
    let files = recorded
        .iter()
        .map(|diff| diff.file.clone())
        .collect::<Vec<_>>()
        .join(", ");
    orch.emit_review_action_log(
        events,
        "info",
        "tool_write",
        &format!("Agent wrote {} file(s) directly", recorded.len()),
        &format!("{}\nUndo Apply restores them.", files),
    );
    events.emit_json(
        "agent-diff-ready",
        serde_json::to_value(&orch.diffs).unwrap_or_default(),
    );
    // 工具写入刚往撤销栈里压了一个 checkpoint，但它本身不改变运行状态，所以要
    // 显式发一次 state：撤销可用性挂在这个事件的 payload 上，不发就意味着运行期间
    // 每次工具写入之后界面上的 Undo 都还停在旧的那个 checkpoint 上。
    events.emit_json("agent-state-changed", orch.state_payload());
}

/// 这次运行临时换了模型时告诉用户。
///
/// 和项目记忆截断一样在运行**开始前**说：它影响这一次运行报出来的金额和预算，而事后再说
/// 用户已经按那个数字下了判断。写 action log 而不是只在聊天框旁边显示一行，是因为那一行
/// 只有正在看设置的人会注意到，而 action log 是"这次运行到底发生了什么"的档案。
async fn emit_model_override_log(
    agent_state: &AgentGlobalState,
    events: &dyn crate::agent::events::RunEvents,
    profile_id: Option<&str>,
    model_override: Option<&str>,
) {
    let Some(model) = model_override.map(str::trim).filter(|m| !m.is_empty()) else {
        return;
    };
    let Ok(config) = agent_state.get_llm_config(profile_id) else {
        return;
    };
    let Some((summary, details)) =
        crate::services::llm_client::model_override_report(&config.model, model)
    else {
        return;
    };
    let orch = agent_state.orchestrator.lock().await;
    orch.emit_run_action_log(events, "warn", "model_override", &summary, &details);
}

/// 项目记忆被截断时告诉用户。
///
/// 为什么在运行**开始前**说，而不是等 `finish_agent_run`：这件事在装配上下文的那一刻才知道，
/// 而它影响的正是即将开始的这一次运行 —— 如果这次运行"没照规矩做"，这条警告就是原因本身，
/// 等结束再说已经晚了一整轮。
///
/// 为它再取一次锁是值得的：`emit_run_action_log` 是同步的，锁不跨 await。
async fn emit_project_memory_warning(
    orchestrator: &tokio::sync::Mutex<crate::agent::orchestrator::AgentOrchestrator>,
    events: &dyn crate::agent::events::RunEvents,
    context: &crate::services::context::AgentContext,
) {
    let Some(bytes) = context.project_memory_truncated else {
        return;
    };
    let (summary, details) = crate::services::project_memory::truncation_report(bytes);
    let orch = orchestrator.lock().await;
    orch.emit_run_action_log(
        events,
        "warn",
        "project_memory_truncated",
        &summary,
        &details,
    );
}

/// 一次运行结束时必须做的三件事，按这个顺序：收尾运行状态、登记工具写入、记账。
///
/// 三个命令共九条退出分支以前各抄一遍这段。抄漏确实发生了：`run_agent_step` 的
/// 取消分支和失败分支都没有记账，于是一个跑到一半被取消的步骤花掉的 token 在
/// action log 里查不到 —— token 已经花了，取消不退款。
///
/// 登记工具写入必须落在每条退出路径上：取消或失败之前发生的写入照样在磁盘上，
/// 不登记就等于磁盘变了而审查区看不到、也没有撤销入口。重复调用是安全的，
/// `take_writes` 会把记录取空，第二次直接返回。
fn finish_agent_run(
    orch: &mut crate::agent::orchestrator::AgentOrchestrator,
    events: &dyn crate::agent::events::RunEvents,
    permissions: &crate::agent::workspace_tools::WorkspaceToolPermissions,
    meter: &crate::services::llm_client::RunUsageMeter,
    llm: &crate::services::llm_client::LlmClient,
    claim: crate::agent::orchestrator::RunClaim,
) {
    orch.finish_run(claim);
    publish_tool_writes(orch, events, permissions);
    publish_external_actions(orch, events, permissions);
    emit_usage_action_log(orch, events, meter);
    emit_degradation_log(orch, events, llm);
}

/// 把这次运行被削减的每一件事写成**一条** action log。
///
/// 以前是五条独立的 `warn`（工具能力、图片、历史修剪、输出夹紧、reasoning 降级），加上运行
/// 前两条，一共七种。七种警告的实际效果是零种：第七条加进去的时候，前六条已经没人看了。
/// 合成一条之后，"这次运行被削了什么"是一个问题、一个答案，明细按需展开。
///
/// 仍然写在这里、而不是各自的成功分支上：降级是**请求已经发出去**的事实，运行最后失败或被
/// 取消并不会把它取消掉，而失败的那次运行恰恰最需要这条线索。
fn emit_degradation_log(
    orch: &AgentOrchestrator,
    events: &dyn crate::agent::events::RunEvents,
    llm: &crate::services::llm_client::LlmClient,
) {
    let mut reports: Vec<(String, String)> = Vec::new();
    if llm.tools_were_rejected() {
        reports.push((
            "tool calling was rejected, so this run fell back to the text protocol".to_string(),
            "The endpoint returned a client error naming the 'tools' parameter, so it was dropped \
             and the request retried. Workspace read tools and MCP tools were unavailable for this \
             run. Set Tool Call Mode to 'Text protocol' for this profile to skip the failed attempt."
                .to_string(),
        ));
    }
    for report in [
        crate::services::llm_client::image_degradation_report(&llm.image_drops()),
        crate::services::llm_client::history_trim_report(&llm.history_trims()),
        crate::services::llm_client::output_clamp_report(&llm.output_clamps()),
        crate::services::llm_client::reasoning_degradation_report(
            llm.reasoning_was_rejected(),
            llm.requested_reasoning_effort(),
        ),
    ]
    .into_iter()
    .flatten()
    {
        reports.push(report);
    }
    if reports.is_empty() {
        return;
    }
    let summary = format!(
        "This run was degraded in {} way(s): {}",
        reports.len(),
        reports
            .iter()
            .map(|(headline, _)| first_clause(headline))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let details = reports
        .iter()
        .map(|(headline, body)| format!("{}\n{}", headline, body))
        .collect::<Vec<_>>()
        .join("\n\n");
    orch.emit_run_action_log(events, "warn", "run_degraded", &summary, &details);
}

/// 取一句话里最前面那一小段，用来拼一行摘要。
///
/// 摘要要能一眼扫完：五条各自的完整句子拼起来有几百字符，而那正是"看起来像噪音"的长度。
fn first_clause(headline: &str) -> String {
    let trimmed = headline.trim();
    let cut = trimmed.find([':', ';']).unwrap_or(trimmed.len());
    let clause = trimmed[..cut].trim();
    let mut chars = clause.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_lowercase(), chars.as_str()),
        None => clause.to_string(),
    }
}

/// 把本次运行的 token 用量写进 action log。措辞和分支判断在
/// `RunUsageSnapshot::action_log_summary` / `action_log_details` 里，那里有测试。
fn emit_usage_action_log(
    orch: &AgentOrchestrator,
    events: &dyn crate::agent::events::RunEvents,
    meter: &crate::services::llm_client::RunUsageMeter,
) {
    let snapshot = meter.snapshot();
    // 一次请求都没发出去就不写这条记录：写一句 "0 calls" 只是噪音。
    if snapshot.calls == 0 {
        return;
    }
    orch.emit_run_action_log(
        events,
        "info",
        "run_token_usage",
        &snapshot.action_log_summary(),
        &snapshot.action_log_details(),
    );
    // 顺带把上下文占用发给界面。测量值和估算值分开走：估算在发送前算、单位是推出来的，
    // 这个是供应商回报的真实 token。没有可测量的东西时 `context_meter()` 返回 None，
    // 界面就什么都不显示 —— 显示 0% 比不显示更糟，它看起来像"上下文几乎是空的"。
    if let Some(meter) = snapshot.context_meter() {
        events.emit_json(
            "agent-context-usage",
            serde_json::to_value(meter).unwrap_or_default(),
        );
    }
}

/// Stop the current Agent task.
#[tauri::command]
pub async fn stop_agent(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    // 先拉开关，再抢锁 —— 顺序不能反。`repair_workspace` 会跨 await 持着
    // orchestrator 锁，先抢锁就得干等到修复自己结束，而那时它已经把开关交回去了，
    // Stop 会拉空，退化成一个只重置界面的空动作。
    agent_state.cancel_registry.cancel_active_run();
    // 挂起的批准请求一并拒掉。少这一句，Stop 之后一个还开着的对话框仍然能放行一次
    // 撤不回的动作 —— 界面已经回到空闲，而导航还是发生了。
    let refused_approvals = agent_state.approval_registry.refuse_all();
    let mut orch = agent_state.orchestrator.lock().await;
    // 报出来而不是只返回一个数：Stop 拒掉的那次动作在工具侧会留一条 `*_cancelled`
    // 记录，但"Stop 替我回答了一个还开着的问题"这件事只有这里知道。
    if refused_approvals > 0 {
        orch.emit_run_action_log(
            &app_handle,
            "warn",
            "external_action",
            &format!(
                "Stop refused {} pending approval request(s)",
                refused_approvals
            ),
            "The action was not performed. Nobody approved it — Stop answered for you.",
        );
    }
    orch.abandon_run();
    orch.state_mgr.set(AgentState::Idle);
    orch.ide_mode = IdeMode::Code;
    // 计划、待审查改动、SDD 草稿都**留着**，只把还在转的那一步落成终态。
    // 理由写在 `note_run_stopped` 上：清掉它们会让"磁盘已经改了、审查区却是空的"成为
    // 按一次 Stop 就能复现的状态。
    let stopped_steps = orch.note_run_stopped(&app_handle);
    if orch.reviewable_diff_count() > 0 || stopped_steps > 0 {
        orch.emit_review_action_log(
            &app_handle,
            "info",
            "run_stopped",
            &format!(
                "Stopped the run; {} pending change(s) and the plan are still here",
                orch.reviewable_diff_count()
            ),
            &format!(
                "Steps marked as stopped: {}\nPending changes kept for review: {}\nStop ends the run, not what it already produced.",
                stopped_steps,
                orch.reviewable_diff_count()
            ),
        );
    }
    Ok("Agent stopped".to_string())
}

/// 把一次批准决定送回给正在等它的工具调用。
///
/// 返回是否真的有人在等：超时之后前端才点到的情况必须能被区分出来，否则界面会显示
/// "已批准"而后端早就把这次动作拒掉了 —— 在这个产品唯一承诺可信的地方说一件没发生的事。
#[tauri::command]
pub async fn resolve_agent_approval(
    request_id: String,
    approved: bool,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<bool, String> {
    Ok(agent_state.approval_registry.resolve(&request_id, approved))
}

/// 把用户对一道选择题的答案送回给正在等它的工具调用。
///
/// 空答案在这里拒掉而不是照送：一个空字符串会让模型收到"用户选了 \"\""，那比没有答案更糟。
/// 前端的"自己写"输入框也靠这条挡住误触发的提交。
#[tauri::command]
pub async fn answer_agent_question(
    request_id: String,
    answer: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<bool, String> {
    let answer = answer.trim();
    if answer.is_empty() {
        return Err("An answer cannot be empty.".to_string());
    }
    Ok(agent_state.approval_registry.answer(&request_id, answer))
}

#[tauri::command]
pub async fn update_agent_step(
    step: TaskStep,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<TaskStep, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let Some(existing) = orch.steps.iter_mut().find(|item| item.id == step.id) else {
        return Err(format!("Step not found: {}", step.id));
    };
    *existing = step.clone();
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "plan_update",
        &format!("Updated step {}", step.title),
        &format!(
            "Step: {}\nScope: {}\nExecution mode: {}",
            step.title,
            step.scope.as_deref().unwrap_or("default"),
            step.execution_mode.as_deref().unwrap_or("default")
        ),
    );
    Ok(step)
}

#[tauri::command]
pub async fn update_agent_steps(
    steps: Vec<TaskStep>,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<TaskStep>, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    orch.steps = steps.clone();
    let _ = app_handle.emit(
        "agent-plan-ready",
        serde_json::to_value(&steps).unwrap_or_default(),
    );
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "plan_update",
        "Updated Agent plan step order",
        &format!(
            "Steps:\n{}",
            steps
                .iter()
                .enumerate()
                .map(|(index, step)| format!("{}. {}", index + 1, step.title))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    );
    Ok(steps)
}

#[tauri::command]
pub async fn skip_agent_step(
    step_id: String,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<TaskStep, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let Some(step) = orch.steps.iter_mut().find(|item| item.id == step_id) else {
        return Err(format!("Step not found: {}", step_id));
    };
    step.status = "skipped".to_string();
    step.logs.push("Skipped by user".to_string());
    let updated = step.clone();
    let _ = app_handle.emit(
        "agent-step-update",
        serde_json::to_value(&updated).unwrap_or_default(),
    );
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "plan_skip",
        &format!("Skipped step {}", updated.title),
        &format!("Step id: {}", updated.id),
    );
    Ok(updated)
}

#[tauri::command]
pub async fn run_agent_step(
    request: RunAgentStepRequest,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
    mcp_state: State<'_, crate::commands::mcp::McpState>,
) -> Result<String, String> {
    let (llm, usage_meter) = agent_state.get_llm_client(
        request.profile_id.as_deref(),
        request.model_override.as_deref(),
    )?;
    let tool_policy =
        crate::services::mcp::McpToolPolicy::from_request(request.tool_approval.as_deref());
    // 开关先造、交给授权，之后一律从授权里取 —— 和 `send_agent_prompt` 同一个理由
    let side_effect_switch = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // 内置只读工作区工具：让模型自己决定读哪些文件，而不是只能吃预打包的上下文
    // 单步执行也走同一套授权：写权限只跟 Auto 模式挂钩
    //
    // 历史不在这里取：它必须和抢执行权同一个临界区，见下面的 `lease` 块。
    let allow_write = {
        let orch = agent_state.orchestrator.lock().await;
        matches!(orch.mode, AgentMode::Auto)
    };
    let mut tool_permissions = agent_tool_permissions(
        request.allow_command_run,
        allow_write,
        request.allow_file_create,
        ExternalGrants {
            allow_browser: request.allow_browser_use,
            browser_origins: request.browser_origins.clone().unwrap_or_default(),
            allow_page_read: request.allow_page_read,
            page_read_origins: request.page_read_origins.clone().unwrap_or_default(),
            allow_computer: request.allow_computer_use,
            computer_apps: request.computer_apps.clone().unwrap_or_default(),
            allow_capture: request.allow_computer_capture,
            capture_apps: request.capture_apps.clone().unwrap_or_default(),
            allow_input: request.allow_computer_input,
            input_apps: request.input_apps.clone().unwrap_or_default(),
        },
        agent_state.approval_gate(&app_handle),
    );
    tool_permissions.adopt_cancel(side_effect_switch);
    // 单步执行也能派子 Agent：见 `send_agent_prompt` 里同一处的理由
    let mut tool_permissions = tool_permissions.with_subagent(
        crate::agent::workspace_tools::SubagentChannel::for_run(&llm),
    );
    let (llm, tool_invoker) = crate::commands::mcp::attach_mcp_tools(
        &mcp_state.registry,
        std::sync::Arc::new(app_handle.clone()),
        llm,
        tool_policy,
        &tool_permissions,
    )
    .await;
    let (llm, tool_invoker) = crate::agent::workspace_tools::attach_workspace_tools(
        llm,
        tool_invoker,
        Some(workspace_tool_logger(&app_handle)),
        tool_permissions.clone(),
    );
    let context_budget = agent_state.get_context_budget(request.profile_id.as_deref());
    let context_sources = request
        .context_sources
        .unwrap_or_else(default_context_sources);
    let compression = resolve_context_compression(
        &agent_state.context_compression,
        request.context_compression.as_deref(),
    )?;
    let step = request.step;
    let step_prompt =
        agent_runtime::format_single_step_prompt(&step, request.extra_prompt.as_deref());

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
    let app_clone = app_handle.clone();
    tokio::spawn(async move {
        while let Some(token) = rx.recv().await {
            let _ = app_clone.emit("agent-stream-token", token);
        }
    });

    let (lease, conversation) = {
        let mut orch = agent_state.orchestrator.lock().await;
        // 单步执行也要抢执行权：它同样改 steps / diffs / 状态机，还会换掉工具面
        // 和记账器。以前这里直接 begin_run，等于绕过守卫从一次流水线运行手里抢走
        // 这些字段。
        let lease = claim_run_for(&mut orch, request.run_id.clone(), &mut tool_permissions)?;
        orch.tool_policy = tool_policy;
        orch.tool_permissions = tool_permissions.clone();
        orch.start_usage_accounting(usage_meter.clone());
        let started = orch.begin_step(&step, "Single step execution started");
        let _ = app_handle.emit(
            "agent-step-update",
            serde_json::to_value(&started).unwrap_or_default(),
        );
        orch.emit_review_action_log(
            &app_handle,
            "info",
            "plan_run_step",
            &format!("Running step {}", step.title),
            &format!(
                "Scope: {}\nExecution mode: {}\nContext mode: {}",
                step.scope.as_deref().unwrap_or("workspace"),
                step.execution_mode.as_deref().unwrap_or("diff"),
                compression
            ),
        );
        // 历史和执行权同一个临界区取，理由见 `send_agent_prompt` 里那一段
        (lease, orch.conversation_digest())
    };
    let claim = lease.claim;
    let cancel_flag = lease.cancel;

    // 单步以前不带历史：用户点"重跑这一步"想表达的"按刚才说的改"在这条路径上
    // 永远丢掉，模型只看到步骤标题。装在抢到执行权之后，抢不到就不必读项目树和 git。
    let mut context = build_agent_context(
        request.active_file,
        request.active_file_content,
        request.selection,
        request.context_files,
        // 流水线的每一步是 Agent 自己在跑，IDE 那一刻的问题面板/终端不是这一步的输入
        None,
        conversation,
    );
    context.enrich_from_workspace_with_sources(&context_sources);
    emit_project_memory_warning(&agent_state.orchestrator, &app_handle, &context).await;
    emit_model_override_log(
        &agent_state,
        &app_handle,
        request.profile_id.as_deref(),
        request.model_override.as_deref(),
    )
    .await;
    let ctx_str = context.to_prompt_context_with_options(&ContextBuildOptions::new(
        compression.clone(),
        context_budget,
    ));

    let response = crate::agent::executor::execute_step(
        &llm,
        &step_prompt,
        &ctx_str,
        tool_invoker.as_deref(),
        cancel_flag,
        tx,
    )
    .await;
    let mut orch = agent_state.orchestrator.lock().await;
    // 登记写入放在分支之前：步骤失败或被取消之前发生的写入照样在磁盘上，
    // 不登记就等于磁盘变了而审查区看不到、也没有撤销入口。
    //
    // 这一步没有并进下面的 `finish_agent_run`：它必须发生在 `record_step_success`
    // 之前，否则工具写入产生的 diff 会被算进"这一步新增了几个 diff"的计数里。
    // `finish_agent_run` 里那次登记因此是空操作，留着是为了别的入口不必记得这条顺序。
    publish_tool_writes(&mut orch, &app_handle, &tool_permissions);
    publish_external_actions(&mut orch, &app_handle, &tool_permissions);
    match response {
        Ok(response) => {
            // 业务逻辑在 orchestrator 里，这里只做加锁 + 事件 + action log
            let outcome = orch.record_step_success(
                &step,
                &response,
                request.regenerated_from_diff_id.as_deref(),
                request.regenerated_from_hunk_index,
            );

            if !outcome.diagnostics.is_empty() {
                orch.emit_review_action_log(
                    &app_handle,
                    "warn",
                    "agent_changes_validation",
                    "Agent changes validation reported issues",
                    &outcome.diagnostics.join("\n"),
                );
            }
            let _ = app_handle.emit(
                "agent-step-update",
                serde_json::to_value(&outcome.step).unwrap_or_default(),
            );
            let _ = app_handle.emit(
                "agent-diff-ready",
                serde_json::to_value(&orch.diffs).unwrap_or_default(),
            );
            // 单步跑完也算一轮：以前只有 `send_agent_prompt` 记历史，于是"生成计划 →
            // 一步步点 Run"这条完整的用法从头到尾不留任何历史 —— 下一句"再改一下"
            // 是冷启动，`verify_workspace` / `repair_workspace` 的"原始任务描述"兜底
            // 也没得可兜。记在 `record_step_success` 之后，结果里才看得到这一步的 diff。
            //
            // 带上这一步自己的贡献：`turn_outcome` 说的是"审查区现在有哪些文件"，
            // 连着跑三步会出现三条一模一样的 result，反而看不出哪一步干了什么。
            orch.record_step_turn(
                &single_step_history_prompt(&step, request.extra_prompt.as_deref()),
                &format!(
                    "this step added {} file diff{}",
                    outcome.new_diffs,
                    if outcome.new_diffs == 1 { "" } else { "s" }
                ),
            );
            finish_agent_run(
                &mut orch,
                &app_handle,
                &tool_permissions,
                &usage_meter,
                &llm,
                claim,
            );
            let _ = app_handle.emit("agent-state-changed", orch.state_payload());
            orch.emit_review_action_log(
                &app_handle,
                "success",
                "plan_run_step",
                &format!(
                    "Step completed with {} new diff{}",
                    outcome.new_diffs,
                    if outcome.new_diffs == 1 { "" } else { "s" }
                ),
                &response,
            );
            Ok("Agent step completed".to_string())
        }
        Err(err) if is_cancelled_error(&err) => {
            finish_agent_run(
                &mut orch,
                &app_handle,
                &tool_permissions,
                &usage_meter,
                &llm,
                claim,
            );
            orch.record_step_status(&step, "todo", "Single step execution cancelled");
            orch.state_mgr.set(AgentState::Idle);
            let _ = app_handle.emit("agent-state-changed", orch.state_payload());
            Ok("Agent task cancelled".to_string())
        }
        Err(err) => {
            finish_agent_run(
                &mut orch,
                &app_handle,
                &tool_permissions,
                &usage_meter,
                &llm,
                claim,
            );
            let failed = orch.record_step_status(&step, "error", &format!("Error: {}", err));
            let _ = app_handle.emit(
                "agent-step-update",
                serde_json::to_value(&failed).unwrap_or_default(),
            );
            orch.state_mgr.set(AgentState::Error(err.clone()));
            let _ = app_handle.emit("agent-state-changed", orch.state_payload());
            orch.emit_review_action_log(
                &app_handle,
                "error",
                "plan_run_step",
                &format!("Step failed {}", step.title),
                &err,
            );
            Err(err)
        }
    }
}

#[tauri::command]
pub async fn continue_agent_pipeline(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
    mcp_state: State<'_, crate::commands::mcp::McpState>,
    profile_id: Option<String>,
    model_override: Option<String>,
) -> Result<String, String> {
    // 续跑必须用**发起时**选的那个 profile 和模型：以前这里写死 `None`，也就是退回当前
    // 活跃 profile —— 用户在聊天里选了模型 B，续跑却悄悄换回 A，价格、上限、窗口全变了，
    // 而界面上没有任何提示。
    let (llm, fresh_meter) =
        agent_state.get_llm_client(profile_id.as_deref(), model_override.as_deref())?;
    // 续跑同样要把"这半段跑在哪个模型上"写进 action log：它是一个独立的 run id，不说的话
    // 那一半运行的金额没有任何出处
    emit_model_override_log(
        &agent_state,
        &app_handle,
        profile_id.as_deref(),
        model_override.as_deref(),
    )
    .await;
    // 续跑是一次新的运行：新开关。沿用暂停前那个开关的话，如果当时是被 Stop 停下的，
    // 续跑会一上来就被自己拦住。
    let side_effect_switch = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // 一个临界区里完成"有暂停的运行吗 -> 抢执行权 -> 取走快照"。顺序不能反：
    // 先取走快照再发现抢不到执行权，那份快照就没了，续跑的唯一凭据被销毁。
    let (paused, tool_policy, tool_permissions, lease, usage_meter) = {
        let mut orch = agent_state.orchestrator.lock().await;
        if orch.paused_run.is_none() {
            return Err("No paused Agent pipeline to continue.".to_string());
        }
        let run_id = orch.last_run_id.clone();
        let mut permissions = orch.tool_permissions.clone();
        permissions.adopt_cancel(side_effect_switch);
        // 续跑按新运行算额度：它有自己的 run id、自己的取消开关、自己的一份用量记账，
        // 图片预算跟着这三样走，而不是跟着"被克隆的那份授权"走
        permissions.reset_image_budget();
        let lease = claim_run_for(&mut orch, run_id, &mut permissions)?;
        let paused = orch
            .paused_run
            .take()
            .expect("paused run checked in this critical section");
        let policy = orch.tool_policy;
        // 续跑必须沿用暂停前的记账器，否则单次运行上限只要中途暂停一次就归零重算。
        // 沿用不到（例如进程重启后恢复）时退回新记账器，而不是干脆不记账。
        //
        // 在这个临界区里就取定，而不是等工具面建好之后：子 Agent 通道拿的是客户端的一份
        // **克隆**，而记账器挂在客户端上。晚挂的话通道里那份还带着 `fresh_meter` —— 一个
        // 没人读的记账器，于是子 Agent 花的钱既不进用量日志，也不受这次运行的上限约束。
        let usage_meter = orch.resumed_usage_meter().unwrap_or(fresh_meter);
        orch.start_usage_accounting(usage_meter.clone());
        orch.emit_review_action_log(
            &app_handle,
            "info",
            "pipeline_continue",
            "Continuing paused Agent pipeline",
            &format!("Continuing from stage {}", paused.stage_index + 1),
        );
        (paused, policy, permissions, lease, usage_meter)
    };
    let claim = lease.claim;
    let llm = llm.with_usage_meter(usage_meter.clone());

    // 续跑要按暂停前的策略重建整个工具面。工具定义（进请求体）和执行器（跑调用）
    // 必须一起装：只装定义会让恢复后的 stage 看到工具，却由上次运行残留的执行器
    // 处理调用，或者根本没人处理。
    //
    // 子 Agent 通道必须**换成这一次的客户端**，不能沿用克隆过来的那个：里面那份客户端带的是
    // 上一次运行的用量记账（子 Agent 花的钱会记到一次已经结束的运行上），还可能带着上一次
    // 的模型覆盖。和上面 `adopt_cancel` / `reset_image_budget` 是同一类必须重置的字段。
    let tool_permissions = tool_permissions.with_subagent(
        crate::agent::workspace_tools::SubagentChannel::for_run(&llm),
    );
    let (llm, tool_invoker) = crate::commands::mcp::attach_mcp_tools(
        &mcp_state.registry,
        std::sync::Arc::new(app_handle.clone()),
        llm,
        tool_policy,
        &tool_permissions,
    )
    .await;
    let (llm, tool_invoker) = crate::agent::workspace_tools::attach_workspace_tools(
        llm,
        tool_invoker,
        Some(workspace_tool_logger(&app_handle)),
        tool_permissions.clone(),
    );

    {
        let mut orch = agent_state.orchestrator.lock().await;
        orch.tool_invoker = tool_invoker;
        // 授权也写回去，和另外三条路径一致：`repair_workspace` 是从这个字段克隆出它的
        // 工具面的，不写回就意味着"暂停 → 续跑 → 修复"里的修复用的是暂停**之前**那份
        // 授权（旧 run id、旧开关）。
        orch.tool_permissions = tool_permissions.clone();
    }

    let stage_index = paused.stage_index;
    // 续跑用的还是暂停前那一问，留一份给历史：暂停那一刻记下的结果是"没有文件改动"，
    // 真正的产出是续跑之后才有的。
    let continued_prompt = paused.prompt.clone();
    let outcome = crate::agent::orchestrator::drive_pipeline(
        &agent_state.orchestrator,
        crate::agent::orchestrator::PipelineRun {
            prompt: paused.prompt,
            ctx_str: paused.context,
            context_summary: paused.context_summary,
            pipeline: paused.pipeline,
            transcript: paused.transcript,
            ide_mode: paused.ide_mode,
        },
        stage_index,
        true,
        lease.cancel,
        &llm,
        std::sync::Arc::new(app_handle.clone()),
    )
    .await;

    let mut orch = agent_state.orchestrator.lock().await;
    match outcome {
        Ok(()) => {
            finish_agent_run(
                &mut orch,
                &app_handle,
                &tool_permissions,
                &usage_meter,
                &llm,
                claim,
            );
            orch.record_continued_turn(&continued_prompt, None);
            Ok("Agent pipeline continued".to_string())
        }
        Err(err) if is_cancelled_error(&err) => {
            finish_agent_run(
                &mut orch,
                &app_handle,
                &tool_permissions,
                &usage_meter,
                &llm,
                claim,
            );
            orch.state_mgr.set(AgentState::Idle);
            let _ = app_handle.emit("agent-state-changed", orch.state_payload());
            Ok("Agent task cancelled".to_string())
        }
        Err(err) => {
            finish_agent_run(
                &mut orch,
                &app_handle,
                &tool_permissions,
                &usage_meter,
                &llm,
                claim,
            );
            Err(err)
        }
    }
}

/// Set the Agent mode.
#[tauri::command]
pub async fn set_agent_mode(
    mode: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<(), String> {
    let parsed = AgentMode::from_str(&mode)?;
    agent_state.orchestrator.lock().await.mode = parsed;
    Ok(())
}

/// Apply all pending diffs to the filesystem.
#[tauri::command]
pub async fn apply_diffs(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<ApplyDiffsResult, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    // 业务逻辑在 orchestrator 里，这里只做加锁 + 事件 + action log
    let result = orch.apply_all_diffs();
    let applied = result.applied.clone();
    let failed = result.failed.clone();

    if failed.is_empty() {
        orch.state_mgr
            .transition(&crate::agent::state_machine::AgentEvent::UserApply);
    } else {
        orch.state_mgr.set(AgentState::WaitingUser);
    }
    let _ = app_handle.emit("agent-state-changed", orch.state_payload());
    orch.emit_review_action_log(
        &app_handle,
        apply_log_level(failed.len()),
        "diff_apply",
        &format!(
            "Apply all diffs: {} applied, {} failed",
            applied.len(),
            failed.len()
        ),
        &format_apply_result_details(&result),
    );

    Ok(result)
}

/// Apply one pending diff to the filesystem.
#[tauri::command]
pub async fn apply_diff(
    diff_id: String,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<ApplyDiffsResult, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    // 业务逻辑在 orchestrator 里，这里只做加锁 + 事件 + action log
    let result = orch.apply_diff(&diff_id)?;
    let failed = result.failed.clone();

    // 审查区状态由 `orch.apply_diff` 自己刷新（`apply_diff_hunk` 同理）。这里
    // 以前又调了一次：无害，但它暗示 orchestrator 不刷新，读的人会照抄到别的
    // 命令里，或者反过来以为这个不变量是命令层维持的。
    let _ = app_handle.emit("agent-state-changed", orch.state_payload());
    orch.emit_review_action_log(
        &app_handle,
        apply_log_level(failed.len()),
        "diff_apply",
        &format!(
            "Apply diff {}: {} applied, {} failed",
            diff_id,
            result.applied.len(),
            failed.len()
        ),
        &format_apply_result_details(&result),
    );

    Ok(result)
}

/// Apply one hunk from a pending diff to the filesystem.
#[tauri::command]
pub async fn apply_diff_hunk(
    diff_id: String,
    hunk_index: usize,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<ApplyDiffsResult, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    // 业务逻辑在 orchestrator 里，这里只做加锁 + 事件 + action log
    let result = orch.apply_diff_hunk(&diff_id, hunk_index)?;
    let failed = result.failed.clone();

    let _ = app_handle.emit("agent-state-changed", orch.state_payload());
    orch.emit_review_action_log(
        &app_handle,
        apply_log_level(failed.len()),
        "diff_apply",
        &format!(
            "Apply hunk {} in diff {}: {} applied, {} failed",
            hunk_index + 1,
            diff_id,
            result.applied.len(),
            failed.len()
        ),
        &format_apply_result_details(&result),
    );

    Ok(result)
}

/// 撤销最近一次应用，把文件恢复到那次应用之前。
///
/// 审查界面只能拒绝还没应用的改动；应用之后此前是单向的，用户只能自己 git。
/// "应用了才发现不对"恰恰是最需要退路的时刻。
#[tauri::command]
pub async fn undo_last_apply(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<crate::agent::orchestrator::UndoResult, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    // 业务逻辑在 orchestrator 里，这里只做加锁 + 事件 + action log
    let result = orch.undo_last_apply()?;

    let _ = app_handle.emit(
        "agent-diff-ready",
        serde_json::to_value(&orch.diffs).unwrap_or_default(),
    );
    // 撤销可用性跟在这个事件的 payload 里，漏掉这一处撤销按钮会静默停在旧值
    let _ = app_handle.emit("agent-state-changed", orch.state_payload());
    orch.emit_review_action_log(
        &app_handle,
        apply_log_level(result.failed.len()),
        "diff_undo",
        &format!(
            "Undid {}: restored {} file(s)",
            result.label,
            result.restored.len()
        ),
        &format!(
            "Restored: {}\nFailed: {}",
            result.restored.join(", "),
            result.failed.join(", ")
        ),
    );

    Ok(result)
}

/// 栈顶回滚点，供界面判断"现在有没有东西可撤销"。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingUndo {
    pub label: String,
    pub files: Vec<String>,
}

/// 查询当前是否有可撤销的应用。**只用于界面首次挂载时取初值。**
///
/// 之后的更新不走查询，而是跟在 `agent-state-changed` 的 payload 里（见
/// `AgentOrchestrator::state_payload`）。原因是这个命令要抢 orchestrator 锁，而
/// `send_agent_prompt` 在整条流水线期间都持有它 —— 查询会排在一次可能跑几分钟的
/// 运行后面。把值放进事件就没有这个问题：它由刚改完撤销栈的同一段代码在同一个
/// 临界区里算出来，既新鲜又不可能漂移。
#[tauri::command]
pub async fn pending_undo(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Option<PendingUndo>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch
        .pending_undo()
        .map(|(label, files)| PendingUndo { label, files }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyWorkspaceRequest {
    /// 要跑的检查命令，按顺序执行
    pub commands: Vec<String>,
    /// 原始任务描述；不给就用最近一轮对话的 prompt
    #[serde(default)]
    pub original_prompt: Option<String>,
}

/// 跑一遍检查命令，失败时给出一段可以直接发出去的修复提示。
///
/// CLI 早就有"跑命令 → 失败喂回模型"的修复循环，桌面端一直没有 —— 桌面端能
/// 生成代码，但不能生成"能通过检查的代码"。这里和 CLI 共用同一套 verification
/// 服务，措辞和截断规则不会各自漂移。
#[tauri::command]
pub async fn verify_workspace(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
    request: VerifyWorkspaceRequest,
) -> Result<crate::services::verification::VerificationReport, String> {
    let (commands, skipped) = crate::services::verification::prepare_commands(request.commands)?;

    let root = workspace::workspace_root()?;
    // 兜底规则见 `resolve_original_prompt`。这里无条件取一次锁（哪怕请求里已经给了
    // 描述）：orchestrator 锁只在短同步片段里持有，代价是一次无竞争的加锁，换来的是
    // 这条规则在三个命令里只有一处实现。
    let last_turn = {
        let orch = agent_state.orchestrator.lock().await;
        orch.last_task_prompt()
    };
    let original_prompt = resolve_original_prompt(request.original_prompt, last_turn);

    let results = crate::services::verification::run_checks(commands, root).await;
    let report = crate::services::verification::summarize(&original_prompt, results, skipped);
    let (level, summary, details) = crate::services::verification::format_report_log(&report);

    let orch = agent_state.orchestrator.lock().await;
    orch.emit_review_action_log(&app_handle, level, "verification_run", &summary, &details);

    Ok(report)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairWorkspaceRequest {
    /// 要跑的检查命令
    pub commands: Vec<String>,
    /// 迭代预算；不给按 1 轮。上限 3 —— 再多几乎只是把预算烧完，
    /// 而每一轮都是一次真实的模型调用加一次落盘。
    #[serde(default)]
    pub max_iterations: Option<u8>,
    /// 原始任务描述；不给就用最近一轮对话的 prompt
    #[serde(default)]
    pub original_prompt: Option<String>,
    /// 用哪个 profile 跑。以前这里根本不传，于是修复循环总是退回当前活跃 profile ——
    /// 用户在聊天里选的模型 B 被悄悄换成 A，价格、上限、窗口跟着变而界面上看不出来。
    #[serde(default)]
    pub profile_id: Option<String>,
    /// 只换模型名，见 `get_llm_client`
    #[serde(default)]
    pub model_override: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairWorkspaceReport {
    pub iterations: u8,
    /// 为什么停：checks passed / iteration budget exhausted / diffs could not be applied
    pub stop_reason: String,
    pub checks_failed: bool,
    pub results: Vec<crate::services::project_tasks::RunProjectTaskResult>,
}

/// 有界修复循环：跑检查 → 失败让模型改 → 落盘 → 再跑检查，直到通过或预算用完。
///
/// 只在 Auto 模式下可用。这不是新的权限层级：Auto 本来就会不经点击直接落盘，
/// 而修复循环的每一轮必须落盘才有意义 —— 改动不进工作区，重跑检查看的还是
/// 原来的代码。suggest / edit 模式承诺"改动先进审查区"，在那里自动落盘会打破
/// 这个承诺，所以直接拒掉而不是悄悄降级成单轮。
#[tauri::command]
pub async fn repair_workspace(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
    mcp_state: State<'_, crate::commands::mcp::McpState>,
    request: RepairWorkspaceRequest,
) -> Result<RepairWorkspaceReport, String> {
    let (commands, _skipped) = crate::services::verification::prepare_commands(request.commands)?;
    let max_iterations = request.max_iterations.unwrap_or(1).clamp(1, 3);
    // 修复循环用发起时选的 profile 和模型，理由同 `continue_agent_pipeline`
    let (llm, usage_meter) = agent_state.get_llm_client(
        request.profile_id.as_deref(),
        request.model_override.as_deref(),
    )?;
    emit_model_override_log(
        &agent_state,
        &app_handle,
        request.profile_id.as_deref(),
        request.model_override.as_deref(),
    )
    .await;
    // 修复也是一次新的运行：新开关，装工具面之前就造好
    let side_effect_switch = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // 只在准备阶段持锁。循环本身由 `drive_repair` 按轮次短持锁 —— 整段持锁会连
    // `get_agent_state` 一起堵住，界面因此永远不知道后端在忙。
    let (lease, original_prompt, repair_permissions, tool_policy) = {
        let mut orch = agent_state.orchestrator.lock().await;
        if !matches!(orch.mode, AgentMode::Auto) {
            return Err(
                "Automatic repair applies its own fixes, so it requires Auto mode.".to_string(),
            );
        }
        // 抢执行权放在模式检查之后：检查不通过就直接返回，先抢会把执行权漏掉。
        // 自动修复会自己往磁盘上落改动，和一次普通运行同等重量，所以必须走同一个守卫。
        let last_run_id = orch.last_run_id.clone();
        // 修复循环这一轮自己的授权：新开关，新 run id。它下面会连工具面一起重建，
        // 否则这个开关就装不到工具那一侧 —— 更糟的是，沿用上一次运行的工具面意味着
        // 沿用它的开关，而那个开关可能被上一次 Stop 永久置成了 true，于是这一次
        // 全新的修复运行会把自己的每一次写盘都拒掉。
        let mut repair_permissions = orch.tool_permissions.clone();
        repair_permissions.adopt_cancel(side_effect_switch);
        // 修复是一次新运行（新 id、新开关），图片额度也要从零算起：带着上一个 prompt
        // 花掉的额度出生，会让第一次读图就被一句假话拒掉
        repair_permissions.reset_image_budget();
        let lease = claim_run_for(&mut orch, last_run_id, &mut repair_permissions)?;
        let tool_policy = orch.tool_policy;
        let original_prompt =
            resolve_original_prompt(request.original_prompt, orch.last_task_prompt());
        orch.start_usage_accounting(usage_meter.clone());
        (lease, original_prompt, repair_permissions, tool_policy)
    };

    // 修复循环也要有自己的工具面。以前它直接沿用上一次运行留在 orchestrator 上的
    // `tool_invoker`：那份授权的副作用开关属于上一次运行，被 Stop 过就永久是 true，
    // 于是这一次修复的每一次写盘都会被拒 —— 一个新运行被上一个运行的 Stop 掐死。
    //
    // 子 Agent 通道同理要换成这一次的客户端：克隆过来的那个带着上一次运行的用量记账。
    let repair_permissions = repair_permissions.with_subagent(
        crate::agent::workspace_tools::SubagentChannel::for_run(&llm),
    );
    let (llm, tool_invoker) = crate::commands::mcp::attach_mcp_tools(
        &mcp_state.registry,
        std::sync::Arc::new(app_handle.clone()),
        llm,
        tool_policy,
        &repair_permissions,
    )
    .await;
    let (llm, tool_invoker) = crate::agent::workspace_tools::attach_workspace_tools(
        llm,
        tool_invoker,
        Some(workspace_tool_logger(&app_handle)),
        repair_permissions.clone(),
    );
    {
        let mut orch = agent_state.orchestrator.lock().await;
        orch.tool_invoker = tool_invoker;
        orch.tool_permissions = repair_permissions.clone();
    }
    let llm = llm.with_usage_meter(usage_meter.clone());

    // 修复接的是同一个任务描述，留一份给历史，见下面 `record_continued_turn`
    let prompt_for_history = original_prompt.clone();
    let outcome = crate::agent::orchestrator::drive_repair(
        &agent_state.orchestrator,
        original_prompt,
        commands,
        crate::services::verification::RepairPolicy::new(max_iterations, true),
        lease.cancel,
        &llm,
        std::sync::Arc::new(app_handle.clone()),
    )
    .await;

    let mut orch = agent_state.orchestrator.lock().await;
    // 修复循环用的是上一次运行留在 orchestrator 上的工具面（`drive_repair` 直接克隆
    // `tool_invoker`），那份授权和这里的 `repair_permissions` 共享同一份日志 `Arc`。
    // 不在这里排空的话，修复轮里发生的写入和导航就要等下一个 prompt —— 而下一个
    // prompt 会换上一份全新的 `Arc`，于是那些记录永远没人取走。
    //
    // 走同一个 `finish_agent_run`，而不是在这里抄一份：这条路径已经因为"自己拼装收尾"
    // 被漏掉三次（ROADMAP 58 漏了排空、70 漏了降级）。收尾里放的是**释放执行权**，
    // 所以它必须落在 `?` 之前，否则修复失败会把执行权永久占住。
    finish_agent_run(
        &mut orch,
        &app_handle,
        &repair_permissions,
        &usage_meter,
        &llm,
        lease.claim,
    );
    let outcome = outcome?;

    // 修复轮次会真的落盘，这件事必须进历史，否则下一句 prompt 看到的还是修复之前的
    // 世界。走 `record_continued_turn`：任务描述和某一轮相同（最常见的情况，描述就是
    // 从那一轮兜底来的）就只更新结果，一轮都对不上才算新的一问。
    //
    // 两种情况**不**记：一次也没迭代（检查一上来就全过了，没有模型调用、没有改动，
    // 记一轮等于在历史里造一个没发生的回合）；描述是那句占位符（它的用处是让人一眼
    // 看出"没有记录"，存成一轮之后它就成了下一次的"原始任务描述"，一句诊断用的话
    // 被洗成了真实任务）。
    if outcome.iterations > 0 && prompt_for_history != ORIGINAL_PROMPT_UNKNOWN {
        orch.record_continued_turn(
            &prompt_for_history,
            Some(&format!(
                "repair ran {} iteration{}",
                outcome.iterations,
                if outcome.iterations == 1 { "" } else { "s" }
            )),
        );
    }

    Ok(RepairWorkspaceReport {
        iterations: outcome.iterations,
        stop_reason: outcome.stop.reason().to_string(),
        checks_failed: outcome.checks_failed,
        results: outcome.results,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairPromptRequest {
    pub command: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    /// 该次运行的输出（stdout / stderr 已由前端合并）
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub original_prompt: Option<String>,
}

/// 为单次失败的命令生成修复提示。
///
/// 存在的意义是消掉重复：这段提示词此前在 Rust（多检查验证）和 TypeScript
/// （单任务 Fix 按钮）各有一份，两边曾对"输出太长时保哪一半"给出相反答案，
/// 而 CLI 用的那半是错的。前端现在优先调这里，TS 那份只留作非 Tauri 兜底。
#[tauri::command]
pub async fn agent_repair_prompt(
    agent_state: State<'_, AgentGlobalState>,
    request: RepairPromptRequest,
) -> Result<String, String> {
    if request.command.trim().is_empty() {
        return Err("Repair prompt needs the failed command.".to_string());
    }

    let last_turn = {
        let orch = agent_state.orchestrator.lock().await;
        orch.last_task_prompt()
    };
    let original_prompt = resolve_original_prompt(request.original_prompt, last_turn);

    let result = crate::services::project_tasks::RunProjectTaskResult {
        command: request.command,
        // 缺失的退出码在 verification 里算作失败，正是这里要的语义
        exit_code: request.exit_code,
        duration_ms: 0,
        stdout: String::new(),
        stderr: request.output,
        problems: Vec::new(),
    };

    Ok(crate::services::verification::build_repair_prompt(
        &original_prompt,
        1,
        std::slice::from_ref(&result),
        &[],
    ))
}

fn is_cancelled_error(err: &str) -> bool {
    err == crate::agent::orchestrator::CANCELLED_ERROR
}

/// Reject all pending diffs.
#[tauri::command]
pub async fn reject_diffs(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<FileDiff>, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    // 业务逻辑在 orchestrator 里，这里只做加锁 + 事件 + action log
    let rejected = orch.reject_all_diffs();

    orch.state_mgr
        .transition(&crate::agent::state_machine::AgentEvent::UserReject);
    let _ = app_handle.emit("agent-state-changed", orch.state_payload());

    orch.emit_review_action_log(
        &app_handle,
        "info",
        "diff_reject",
        &format!(
            "Rejected {} diff{}",
            rejected.len(),
            if rejected.len() == 1 { "" } else { "s" }
        ),
        &format_diff_list_details(&rejected),
    );

    Ok(rejected)
}

/// Reject one pending diff.
#[tauri::command]
pub async fn reject_diff(
    diff_id: String,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<FileDiff, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    // 业务逻辑在 orchestrator 里，这里只做加锁 + 事件 + action log
    let rejected = orch.reject_diff(&diff_id)?;

    let _ = app_handle.emit("agent-state-changed", orch.state_payload());
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "diff_reject",
        &format!("Rejected diff {}", diff_id),
        &format_diff_list_details(std::slice::from_ref(&rejected)),
    );

    Ok(rejected)
}

/// Reject one hunk from a pending diff.
#[tauri::command]
pub async fn reject_diff_hunk(
    diff_id: String,
    hunk_index: usize,
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<FileDiff, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    // 业务逻辑在 orchestrator 里，这里只做加锁 + 事件 + action log
    let updated = orch.reject_diff_hunk(&diff_id, hunk_index)?;

    let _ = app_handle.emit("agent-state-changed", orch.state_payload());
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "diff_reject",
        &format!("Rejected hunk {} in diff {}", hunk_index + 1, diff_id),
        &format_diff_list_details(std::slice::from_ref(&updated)),
    );

    Ok(updated)
}

fn format_apply_result_details(result: &ApplyDiffsResult) -> String {
    let mut lines = Vec::new();
    for diff in &result.applied {
        lines.push(format!("Applied: {} ({})", diff.file, diff.id));
    }
    for failure in &result.failed {
        lines.push(format!(
            "Failed: {} ({}) - {}",
            failure.file, failure.diff_id, failure.message
        ));
    }
    if lines.is_empty() {
        "No matching pending diffs were changed.".to_string()
    } else {
        lines.join("\n")
    }
}

fn format_diff_list_details(diffs: &[FileDiff]) -> String {
    if diffs.is_empty() {
        return "No diffs.".to_string();
    }
    diffs
        .iter()
        .map(|diff| {
            format!(
                "{} ({}) - {} hunk{}",
                diff.file,
                diff.id,
                diff.hunks.len(),
                if diff.hunks.len() == 1 { "" } else { "s" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 单步执行进历史时记的是"人能看懂的那一句"，不是喂给模型的那份模板。
///
/// `format_single_step_prompt` 产出的是带步骤类型、范围、执行模式和输出规则的整段
/// 提示词。历史每轮只有 400 字，全填模板等于把这个窗口浪费掉。
///
/// 补充说明也要限长，而且只取第一行：`regenerateDiff` 传进来的 `extraPrompt` 是
/// 一整段 hunk JSON 加整份文件内容，原样记下来就是往历史里塞一坨源码，之后每次
/// prompt 都会把它再发一遍 —— 和记模板是同一个错，只是从另一个参数进来。
fn single_step_history_prompt(step: &TaskStep, extra: Option<&str>) -> String {
    let extra = extra
        .and_then(|value| value.lines().find(|line| !line.trim().is_empty()))
        .map(str::trim)
        .map(|line| {
            if line.chars().count() > MAX_STEP_EXTRA_CHARS {
                let head: String = line.chars().take(MAX_STEP_EXTRA_CHARS).collect();
                format!("{}…", head)
            } else {
                line.to_string()
            }
        });
    match extra {
        Some(extra) => format!("Ran step: {} — {}", step.title, extra),
        None => format!("Ran step: {}", step.title),
    }
}

/// 步骤补充说明进历史时的字符上限。留得比 400 小得多：这一行的用处是"当时还额外
/// 交代了什么"，步骤标题才是主语。
const MAX_STEP_EXTRA_CHARS: usize = 100;

/// 对话历史是**必填参数**，不是可以忘的字段。
///
/// 以前它在这里写死 `None`，由各调用点自己补：`send_agent_prompt` 补了，估算和
/// 单步都没补。后果不是少一段文字，而是同一份上下文在"要花多少 token"和"真的
/// 发出去多少 token"上给出两个数，聊得越久差得越多。签名里留一个坑，三条路径里
/// 就有两条掉进去；改成参数之后，漏掉的那条编译不过。
fn build_agent_context(
    active_file: Option<String>,
    active_file_content: Option<String>,
    selection: Option<String>,
    context_files: Vec<String>,
    ide_runtime: Option<String>,
    conversation: Option<String>,
) -> AgentContext {
    AgentContext {
        active_file,
        active_file_content,
        selection,
        open_files: context_files,
        project_path: workspace::workspace_root_string(),
        git_diff: None,
        project_tree: None,
        project_memory: None,
        project_memory_truncated: None,
        conversation,
        ide_runtime,
    }
}

fn default_context_sources() -> ContextSourceOptions {
    ContextSourceOptions {
        include_project_tree: true,
        include_git_diff: true,
        include_project_memory: true,
    }
}

/// 请求里指定的压缩模式优先，没指定才用设置里存的默认值。
///
/// 参数收的是那把锁本身而不是 `State<AgentGlobalState>`：这个函数只读一个字段，
/// 而只要签名里写着 `State`，它就只能靠启动整个应用来验证。
fn resolve_context_compression(
    stored: &std::sync::Mutex<ContextCompressionMode>,
    requested: Option<&str>,
) -> Result<ContextCompressionMode, String> {
    match requested {
        Some(mode) => ContextCompressionMode::from_str(mode),
        None => stored
            .lock()
            .map_err(|e| e.to_string())
            .map(|mode| mode.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::events::RecordingEvents;
    use crate::agent::executor::ToolInvoker;
    use crate::agent::orchestrator::AgentOrchestrator;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    /// 测试里的批准通道：没人应答，所以每次请求都会走到超时。
    ///
    /// 超时设得很短是为了让"忘了应答"的测试不至于挂两分钟；不给 `None` 是因为生产
    /// 路径上一定有通道，测试要走的是同一条路。
    /// 这条事件是界面上那一行"测到的占用"的唯一来源，而它的键名是驼峰序列化出来的。
    /// 少了 `#[serde(rename_all)]`，前端的归一化会把整个载荷判成无效，那一行就永远
    /// 不出现 —— 五条命令全绿，界面静默少一块。所以这里连键名一起钉住。
    #[test]
    fn the_context_meter_reaches_the_frontend_with_the_keys_it_expects() {
        let orch = AgentOrchestrator::new();
        let events = RecordingEvents::new();
        let meter = crate::services::llm_client::RunUsageMeter::new(None);

        // 没发过请求：连 action log 都不该有，更不该有占用
        emit_usage_action_log(&orch, &events, &meter);
        assert_eq!(events.count("agent-context-usage"), 0);

        // 发了但供应商不报用量：有 action log，没有占用
        meter.record_call();
        meter.record_usage(Some(&crate::services::llm_client::LlmUsage::default()));
        emit_usage_action_log(&orch, &events, &meter);
        assert_eq!(events.count("agent-context-usage"), 0);

        meter.record_usage(Some(&crate::services::llm_client::LlmUsage {
            prompt_tokens: Some(12_000),
            completion_tokens: Some(500),
            total_tokens: None,
            ..Default::default()
        }));
        emit_usage_action_log(&orch, &events, &meter);
        let payload = events
            .payloads_for("agent-context-usage")
            .pop()
            .expect("context usage event");
        assert_eq!(payload["peakTotalTokens"], 12_500);
    }

    /// 会话列表的键名也是驼峰序列化出来的，前端 `normalizeAgentSessionList` 按名字取。
    ///
    /// 少了 `#[serde(rename_all)]`，每一行都会渲染成 "unknown time · 0 turns"、没有一行被标成
    /// current，而五条验证命令全绿 —— 和 `agent-context-usage` 当初那个缺陷一模一样，所以这里
    /// 同样按名字钉住。
    #[test]
    fn the_session_list_reaches_the_frontend_with_the_keys_it_expects() {
        let list = AgentSessionList {
            active_id: "session-1".to_string(),
            sessions: vec![AgentSessionSummary {
                id: "session-1".to_string(),
                title: "fix the parser".to_string(),
                updated_at: 1_700_000_000_000,
                turn_count: 3,
                last_outcome: "2 file(s) applied".to_string(),
            }],
            warning: None,
            sessions_are_saved: true,
        };

        let payload = serde_json::to_value(&list).expect("serialize");
        assert_eq!(payload["activeId"], "session-1");
        assert_eq!(payload["sessionsAreSaved"], true);
        assert_eq!(payload["sessions"][0]["updatedAt"], 1_700_000_000_000u64);
        assert_eq!(payload["sessions"][0]["turnCount"], 3);
        assert_eq!(payload["sessions"][0]["lastOutcome"], "2 file(s) applied");

        // 恢复的返回值同理：`turns` 里每一轮的键名是前端重建聊天区的依据
        let detail = AgentSessionDetail {
            id: "session-1".to_string(),
            title: "fix the parser".to_string(),
            turns: vec![crate::agent::orchestrator::ConversationTurn {
                id: "turn-1".to_string(),
                prompt: "fix the parser".to_string(),
                outcome: "no file changes produced".to_string(),
                derived: true,
                run_id: Some("run-1".to_string()),
            }],
        };
        let payload = serde_json::to_value(&detail).expect("serialize");
        assert_eq!(payload["turns"][0]["id"], "turn-1");
        assert_eq!(payload["turns"][0]["derived"], true);
    }

    fn test_approval_gate() -> crate::agent::approval::ApprovalGate {
        crate::agent::approval::ApprovalGate::new(
            crate::agent::approval::ApprovalRegistry::new(),
            Arc::new(RecordingEvents::new()),
        )
        .with_timeout(std::time::Duration::from_millis(50))
    }

    /// action log 条目里 `summary` 那一段。断言措辞是有意的：这一层的缺陷全都是措辞
    /// 缺陷 —— "0 个动作" 曾经被报成 "1 browser action(s) … cannot be undone"。
    fn action_log_summaries(events: &RecordingEvents) -> Vec<(String, String, String)> {
        events
            .payloads_for("agent-action-log")
            .into_iter()
            .map(|payload| {
                (
                    payload["level"].as_str().unwrap_or_default().to_string(),
                    payload["phase"].as_str().unwrap_or_default().to_string(),
                    payload["summary"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }

    fn mock_llm(model: &str, endpoint: &str) -> crate::services::llm_client::LlmClient {
        crate::services::llm_client::LlmClient::new(crate::services::llm_client::LlmConfig {
            endpoint: endpoint.to_string(),
            api_key: "sk-test".to_string(),
            model: model.to_string(),
            provider: "openai".to_string(),
            max_context_tokens: None,
            reasoning_effort: None,
            max_output_tokens: None,
            tool_call_mode: "text_protocol".to_string(),
            model_type: crate::services::llm_client::ModelType::OpenAI,
            local_model_config: None,
        })
    }

    /// 授权和租约必须拿着**同一个**副作用开关。
    ///
    /// 以前这是两行相邻的代码、四条命令各抄一遍：一行交给授权，一行交给租约，两处传的
    /// 完全可以是不同的 `Arc` 而编译器不会说一个字。传错的后果是 Stop 之后工具照跑，
    /// 而 ROADMAP 60 正是把"没有东西检查这条接线"记为未覆盖。
    #[test]
    fn the_lease_and_the_permissions_share_one_cancel_switch() {
        let switch = Arc::new(AtomicBool::new(false));
        let mut permissions =
            crate::agent::workspace_tools::WorkspaceToolPermissions::new(Vec::new(), false, false);
        permissions.adopt_cancel(switch.clone());
        let mut orch = AgentOrchestrator::new();

        let lease = claim_run_for(&mut orch, Some("run-9".to_string()), &mut permissions)
            .expect("a fresh orchestrator hands out the lease");

        assert!(
            Arc::ptr_eq(&lease.cancel, &switch),
            "the lease must carry the switch the tool surface already holds"
        );
        // 记录要认领这一次运行，而不是上一次
        assert!(permissions.run_id.is_some());
        assert_eq!(permissions.run_id, orch.current_run_id);

        // Stop 拉的是租约那一份，工具那一侧必须立刻看见
        lease
            .cancel
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(permissions.cancelled());
    }

    /// 被 Stop 拦下的浏览器调用要进记录，但**不能**被算成"已经发生"。
    ///
    /// 这条走的是真实路径：授权齐备 + 开关已拉，`invoke` 自己写下
    /// `browser_open_cancelled`，然后由命令层来数。以前这一层把 `_cancelled` 算作
    /// performed，于是一次被拦下的导航会在唯一可信的那块地方被写成"已经做了，撤不回"。
    ///
    /// 它同时是落盘那条线唯一的端到端覆盖：`publish_external_actions` 现在还要把记录写
    /// 进磁盘，而删掉那一行时所有单元测试都还是绿的 —— "持久化其实从没发生过"正是这个
    /// 模块的注释里写着要防的事。所以这里自己准备一个配置目录，并在最后去看那个文件。
    #[test]
    fn a_stopped_browser_call_is_recorded_but_not_counted_as_performed() {
        let _guard = crate::services::workspace::env_test_guard();
        let config_dir = std::env::temp_dir().join(format!(
            "agent-ide-publish-external-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&config_dir).expect("建测试配置目录");
        std::env::set_var("AGENT_IDE_CONFIG_DIR", &config_dir);
        crate::services::workspace::save_workspace_path("D:/work/publish-external")
            .expect("保存工作区");

        let mut permissions =
            crate::agent::workspace_tools::WorkspaceToolPermissions::new(Vec::new(), false, false)
                .with_browser(true, vec!["https://example.com".to_string()]);
        permissions.adopt_cancel(Arc::new(AtomicBool::new(true)));

        let invoker = crate::agent::workspace_tools::WorkspaceToolInvoker::new(
            Arc::new(|_: &str, _: &str, _: &str| {}),
            permissions.clone(),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime is enough for a refused call");
        let result = runtime.block_on(invoker.invoke(
            crate::agent::workspace_tools::BROWSER_OPEN,
            "{\"url\":\"https://example.com/pricing\"}",
        ));
        assert!(result.is_err(), "a stopped run must not navigate");

        let mut orch = AgentOrchestrator::new();
        let events = RecordingEvents::new();
        publish_external_actions(&mut orch, &events, &permissions);

        let logs = action_log_summaries(&events);
        assert_eq!(logs.len(), 1, "{:?}", logs);
        assert_eq!(logs[0].0, "warn");
        assert_eq!(logs[0].1, "external_action");
        assert!(
            logs[0].2.contains("performed 0 external action(s)"),
            "{}",
            logs[0].2
        );
        assert!(
            logs[0].2.contains("1 refused, failed or stopped"),
            "{}",
            logs[0].2
        );
        // 记录本身要留在 orchestrator 上，否则前端刷新后这次尝试就查不到了
        assert_eq!(orch.external_actions.len(), 1);
        assert_eq!(orch.external_actions[0].kind, "browser_open_cancelled");
        // 而且要落到盘上：只活到关窗为止的审计记录，在用户真正需要它那天正好是空的
        let persisted = std::fs::read_to_string(config_dir.join("external-actions.json"))
            .expect("落盘那一行被删掉时这里就读不到文件");
        assert!(
            persisted.contains("browser_open_cancelled"),
            "{}",
            persisted
        );
        assert!(
            persisted.contains("D:/work/publish-external"),
            "{}",
            persisted
        );

        std::env::remove_var("AGENT_IDE_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    /// 图片降级必须跟着运行收尾一起送出去，没有降级时则一个字都不说。
    ///
    /// 这条钉的是 `finish_agent_run` 本身的行为。它是四个运行命令**唯一**的收尾入口
    /// （包括 `repair_workspace`，那条路径以前自己拼装，被漏掉过两次），所以"失败和
    /// 取消也报"这件事靠的是那唯一入口，而不是这条测试 —— 调用点本身仍然要靠读代码。
    #[test]
    fn finishing_a_run_reports_a_dropped_image_and_stays_quiet_otherwise() {
        let mut permissions =
            crate::agent::workspace_tools::WorkspaceToolPermissions::new(Vec::new(), false, false);
        // 走真实路径：开关在这里铸造并交给授权，`try_begin_run` 再从授权里取。
        // 直接用 `Default` 那个开关也能过断言，但那条路生产代码从不走。
        permissions.adopt_cancel(Arc::new(AtomicBool::new(false)));
        let meter = crate::services::llm_client::RunUsageMeter::new(None);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime is enough: nothing here waits on I/O");

        // mock 端点把消息拍平成文本，图片没有位置可去 —— 真实的降级路径
        let dropped = mock_llm("gpt-4o", "mock://images");
        // rx 必须活到请求结束：mock 流会往里发 token，收端一关请求就报错
        let (tx, _rx) = tokio::sync::mpsc::channel::<String>(8);
        let sent = runtime.block_on(dropped.stream_chat_with_tools(
            vec![
                crate::services::llm_client::ChatMessage::user("look at this").with_images(vec![
                    crate::services::images::ImagePart {
                        media_type: "image/png".to_string(),
                        base64_data: "Zm9vYmFy".to_string(),
                    },
                ]),
            ],
            Arc::new(AtomicBool::new(false)),
            tx,
        ));
        assert!(sent.is_ok(), "{:?}", sent);

        let mut orch = AgentOrchestrator::new();
        let lease = orch
            .try_begin_run(Some("run-1".to_string()), &permissions)
            .expect("a fresh orchestrator hands out the lease");
        let events = RecordingEvents::new();
        finish_agent_run(
            &mut orch,
            &events,
            &permissions,
            &meter,
            &dropped,
            lease.claim,
        );
        let phases = action_log_summaries(&events)
            .into_iter()
            .map(|(_, phase, summary)| (phase, summary))
            .collect::<Vec<_>>();
        assert!(
            phases.iter().any(|(phase, summary)| phase == "run_degraded"
                && summary.contains("degraded in 1 way(s)")
                && summary.contains("1 image(s) were not sent to the model")),
            "{:?}",
            phases
        );
        // 运行级别的事实不能挂在某个阶段上：挂成 "Diff Review" 的话，一条"图片没发出去"
        // 会出现在评审阶段下面，而真正丢图的那个阶段什么都不显示。
        let stages: Vec<serde_json::Value> = events
            .payloads_for("agent-action-log")
            .into_iter()
            .filter(|payload| payload["phase"] == "run_degraded")
            .map(|payload| payload["stage"].clone())
            .collect();
        assert_eq!(stages, vec![serde_json::Value::Null], "{:?}", stages);

        // 没有降级的运行不能写这条：一条永远出现的警告等于没有警告。这里要真的走一遍
        // 同一个降级路径（同一个 mock 端点，只是消息里没有图），否则断言就只是在说
        // "没调用过的 client 没有记录"，那是句废话。
        let clean = mock_llm("gpt-4o", "mock://images");
        let (tx, _rx) = tokio::sync::mpsc::channel::<String>(8);
        let sent = runtime.block_on(clean.stream_chat_with_tools(
            vec![crate::services::llm_client::ChatMessage::user(
                "no picture here",
            )],
            Arc::new(AtomicBool::new(false)),
            tx,
        ));
        assert!(sent.is_ok(), "{:?}", sent);
        let mut orch = AgentOrchestrator::new();
        let lease = orch
            .try_begin_run(Some("run-2".to_string()), &permissions)
            .expect("a fresh orchestrator hands out the lease");
        let events = RecordingEvents::new();
        finish_agent_run(
            &mut orch,
            &events,
            &permissions,
            &meter,
            &clean,
            lease.claim,
        );
        assert!(
            !action_log_summaries(&events)
                .iter()
                .any(|(_, phase, _)| phase == "run_degraded"),
            "{:?}",
            action_log_summaries(&events)
        );
    }
    // status_from_hunks 已随业务逻辑搬到 orchestrator，命令层只剩适配代码
    use crate::agent::orchestrator::status_from_hunks;

    /// 部分失败**绝不能**记成 success：这个产品的前提是用户看得清 Agent 做了什么，
    /// 一次有文件没改到的应用被标成成功，用户就会以为改动全落盘了。至于失败该记
    /// warn 还是 error 是可以改的选择，所以断言落在"必须是一个引人注意的级别"上，
    /// 而不是某个具体字符串。
    #[test]
    fn a_partial_failure_is_never_logged_as_success() {
        assert_eq!(apply_log_level(0), "success");
        assert!(matches!(apply_log_level(1), "warn" | "error"));
        assert!(matches!(apply_log_level(7), "warn" | "error"));
    }

    /// 请求里给了原始任务描述就用它，不去翻对话记录。
    #[test]
    fn an_explicit_original_prompt_wins_over_the_conversation() {
        assert_eq!(
            resolve_original_prompt(
                Some("add pagination".to_string()),
                Some("something older".to_string())
            ),
            "add pagination"
        );
    }

    /// 前端输入框留空送来的是 `Some("")`，不是 `None`。一个空的原始任务描述会让
    /// 模型只看到一堆报错、不知道要修成什么样，所以空白必须按"没给"处理。
    #[test]
    fn a_blank_request_falls_back_to_the_last_turn() {
        assert_eq!(
            resolve_original_prompt(Some("   ".to_string()), Some("fix the parser".to_string())),
            "fix the parser"
        );
        assert_eq!(
            resolve_original_prompt(None, Some("fix the parser".to_string())),
            "fix the parser"
        );
    }

    /// 什么都没有时给一句可诊断的占位符，而不是空串：提示里出现
    /// "(original task not recorded)" 能看出发生了什么，一段空白看不出。
    #[test]
    fn a_missing_prompt_is_marked_rather_than_left_empty() {
        let resolved = resolve_original_prompt(None, None);
        assert!(!resolved.trim().is_empty());
        assert_eq!(resolved, "(original task not recorded)");
    }

    fn task(command: &str) -> crate::services::project_tasks::ProjectTask {
        crate::services::project_tasks::ProjectTask {
            id: command.to_string(),
            label: command.to_string(),
            command: command.to_string(),
            source: "package.json".to_string(),
            description: String::new(),
        }
    }

    /// 授权是一票否决：没勾"允许执行命令"就一条都不给，哪怕项目声明了一堆任务。
    /// 这条以前埋在一个 `if/else` 表达式里，只能靠跑桌面应用验证。
    #[test]
    fn no_command_is_allowed_without_authorization() {
        let tasks = vec![task("npm test"), task("cargo test")];
        assert!(allowed_agent_commands(tasks, false).is_empty());
    }

    /// 长驻命令必须落在清单外：验证是"跑完看结果"，而 `npm run dev` 不会退出，
    /// 给了它等于给 Agent 一个能把整次运行挂死的工具。
    #[test]
    fn long_running_commands_are_dropped_from_the_allow_list() {
        let allowed = allowed_agent_commands(
            vec![
                task("npm test"),
                task("npm run dev"),
                task("cargo build"),
                task("npm run watch"),
            ],
            true,
        );
        assert_eq!(
            allowed,
            vec!["npm test".to_string(), "cargo build".to_string()]
        );
    }

    /// 三个 bool 仍然靠位置传，换一下顺序照样编译得过。这条把它们各自落到哪个字段
    /// 钉住：只允许写盘时，命令清单必须是空的，新建文件必须仍然不允许。四档外部授权
    /// 走具名字段，这里钉的是"它们彼此不互相蕴含"。
    #[test]
    fn each_flag_lands_on_its_own_permission() {
        let write_only = agent_tool_permissions(
            false,
            true,
            false,
            ExternalGrants::default(),
            test_approval_gate(),
        );
        assert!(write_only.allowed_commands.is_empty());
        assert!(write_only.allow_write);
        assert!(!write_only.allow_create);
        assert!(!write_only.allow_browser);

        let create_only = agent_tool_permissions(
            false,
            false,
            true,
            ExternalGrants::default(),
            test_approval_gate(),
        );
        assert!(!create_only.allow_write);
        assert!(create_only.allow_create);

        // 浏览器授权和写盘授权互不牵连：给了浏览器不等于能改文件
        let browser_only = agent_tool_permissions(
            false,
            false,
            false,
            ExternalGrants {
                allow_browser: true,
                browser_origins: vec!["http://127.0.0.1:1420".to_string()],
                ..ExternalGrants::default()
            },
            test_approval_gate(),
        );
        assert!(!browser_only.allow_write);
        assert!(browser_only.allow_browser);
        assert_eq!(browser_only.browser_origins.len(), 1);
        // 桌面观察也是独立的一档：给了浏览器不等于能看桌面
        assert!(!browser_only.allow_computer);
        // 能**打开**一个页面也不等于能**读**它的正文：正文里有登录之后才看得到的东西
        assert!(!browser_only.allow_page_read);
        assert!(browser_only.page_read_origins.is_empty());

        let page_read_only = agent_tool_permissions(
            false,
            false,
            false,
            ExternalGrants {
                allow_page_read: true,
                page_read_origins: vec!["https://example.com".to_string()],
                ..ExternalGrants::default()
            },
            test_approval_gate(),
        );
        assert!(page_read_only.allow_page_read);
        assert_eq!(page_read_only.page_read_origins.len(), 1);
        // 反过来同理：允许读一份已经打开的文档，不等于允许它去开新页面
        assert!(!page_read_only.allow_browser);
        assert!(page_read_only.browser_origins.is_empty());

        let computer_only = agent_tool_permissions(
            false,
            false,
            false,
            ExternalGrants {
                allow_computer: true,
                computer_apps: vec!["Code.exe".to_string()],
                ..ExternalGrants::default()
            },
            test_approval_gate(),
        );
        assert!(computer_only.allow_computer);
        assert_eq!(computer_only.computer_apps.len(), 1);
        assert!(!computer_only.allow_browser);
        assert!(!computer_only.allow_write);
        // 看得到窗口列表**不等于**能截窗口内容：截图是独立的一档授权，
        // 而它是这两档里唯一会把窗口里的内容交出去的那个
        assert!(!computer_only.allow_capture);
        assert!(computer_only.capture_apps.is_empty());

        let capture_only = agent_tool_permissions(
            false,
            false,
            false,
            ExternalGrants {
                allow_capture: true,
                capture_apps: vec!["Code.exe".to_string()],
                ..ExternalGrants::default()
            },
            test_approval_gate(),
        );
        assert!(capture_only.allow_capture);
        assert_eq!(capture_only.capture_apps.len(), 1);
        // 反过来也成立：给了截图不等于顺带给了窗口枚举
        assert!(!capture_only.allow_computer);
        assert!(capture_only.computer_apps.is_empty());
        // 也不等于给了**动手**：读窗口内容和往窗口里点是两件不同性质的事，后者撤不回
        assert!(!capture_only.allow_input);
        assert!(capture_only.input_apps.is_empty());

        let input_only = agent_tool_permissions(
            false,
            false,
            false,
            ExternalGrants {
                allow_input: true,
                input_apps: vec!["Code.exe".to_string()],
                ..ExternalGrants::default()
            },
            test_approval_gate(),
        );
        assert!(input_only.allow_input);
        assert_eq!(input_only.input_apps.len(), 1);
        // 点击那一档也不会把截图或枚举一起带出来
        assert!(!input_only.allow_capture);
        assert!(input_only.capture_apps.is_empty());
        assert!(!input_only.allow_computer);
    }

    /// 请求里给了模式就用它，没给才回落到设置里的默认值。
    /// 这两条以前只能靠跑桌面应用才验证得到，因为函数签名收的是 `State`。
    #[test]
    fn context_compression_request_overrides_the_stored_default() {
        let stored = std::sync::Mutex::new(ContextCompressionMode::Focused);

        assert_eq!(
            resolve_context_compression(&stored, Some("compact")).unwrap(),
            ContextCompressionMode::Compact
        );
        assert_eq!(
            resolve_context_compression(&stored, None).unwrap(),
            ContextCompressionMode::Focused
        );
        // 无法识别的模式是错误，而不是悄悄退回默认值：那会让一次预期外的打包
        // 看起来完全正常
        assert!(resolve_context_compression(&stored, Some("nonsense")).is_err());
    }

    fn test_hunk(status: Option<&str>) -> crate::agent::state_machine::DiffHunk {
        crate::agent::state_machine::DiffHunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            content: "line".to_string(),
            original: "line".to_string(),
            updated: "line".to_string(),
            provenance: None,
            status: status.map(|value| value.to_string()),
        }
    }

    #[test]
    fn hunk_status_rollup_keeps_partial_reviewable() {
        assert_eq!(
            status_from_hunks(&[test_hunk(Some("applied")), test_hunk(None)]),
            "partial"
        );
        assert_eq!(
            status_from_hunks(&[test_hunk(Some("applied")), test_hunk(Some("rejected"))]),
            "partial"
        );
        assert_eq!(
            status_from_hunks(&[test_hunk(Some("rejected")), test_hunk(Some("rejected"))]),
            "rejected"
        );
        assert_eq!(
            status_from_hunks(&[test_hunk(Some("applied")), test_hunk(Some("applied"))]),
            "applied"
        );
        assert_eq!(
            status_from_hunks(&[test_hunk(Some("failed")), test_hunk(None)]),
            "failed"
        );
    }

    #[test]
    fn step_prompt_includes_scope_and_mode() {
        let step = TaskStep {
            id: "s1".to_string(),
            title: "Fix parser".to_string(),
            step_type: "edit".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
            scope: Some("active_file".to_string()),
            execution_mode: Some("fix".to_string()),
        };

        let prompt = agent_runtime::format_single_step_prompt(&step, Some("Use more context"));

        assert!(prompt.contains("Fix parser"));
        assert!(prompt.contains("Scope: active_file"));
        assert!(prompt.contains("Execution mode: fix"));
        assert!(prompt.contains("Use more context"));
    }

    /// 历史里那一行是给人和下一轮模型看的一句话，不能是整段提示词模板，
    /// 也不能是调用方顺手塞进来的一整份文件。
    #[test]
    fn a_step_enters_history_as_a_bounded_sentence() {
        let step = TaskStep {
            id: "s1".to_string(),
            title: "Fix parser".to_string(),
            step_type: "edit".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
            scope: Some("active_file".to_string()),
            execution_mode: Some("fix".to_string()),
        };

        let recorded = single_step_history_prompt(&step, Some("Use more context"));
        assert_eq!(recorded, "Ran step: Fix parser — Use more context");
        // 模板里的字段名不该进历史：它们占的是那 400 字的额度
        assert!(!recorded.contains("Execution mode"), "{}", recorded);

        // `regenerateDiff` 传的是 hunk JSON 加整份文件内容。只留第一行、且限长，
        // 否则历史里会存一坨源码，而且之后每次 prompt 都把它再发一遍。
        let slab = format!(
            "Regenerate these hunks:\n{}\nCurrent file content:\n{}",
            "{ \"original\": \"const a = 1;\" }",
            "const a = 1;\n".repeat(200)
        );
        let bounded = single_step_history_prompt(&step, Some(&slab));
        assert!(
            bounded.chars().count() < 150,
            "{} chars: {}",
            bounded.chars().count(),
            bounded
        );
        assert!(
            bounded.starts_with("Ran step: Fix parser — "),
            "{}",
            bounded
        );
        assert!(!bounded.contains("Current file content"), "{}", bounded);

        // 没有补充说明时不留一个空的分隔符
        assert_eq!(
            single_step_history_prompt(&step, Some("   ")),
            single_step_history_prompt(&step, None)
        );
        assert_eq!(
            single_step_history_prompt(&step, None),
            "Ran step: Fix parser"
        );
    }

    #[test]
    fn step_provenance_records_regeneration_source() {
        let step = TaskStep {
            id: "s1".to_string(),
            title: "Regenerate stale hunk".to_string(),
            step_type: "edit".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
            scope: None,
            execution_mode: None,
        };
        let mut diffs = vec![FileDiff {
            id: "d2".to_string(),
            file: "src/app.ts".to_string(),
            base_hash: None,
            provenance: None,
            hunks: Vec::new(),
            status: "pending".to_string(),
        }];

        agent_runtime::attach_step_provenance(&mut diffs, &step, Some("d1"), Some(2));

        let provenance = diffs[0].provenance.as_ref().expect("provenance");
        assert_eq!(provenance.source_role.as_deref(), Some("agent-step"));
        assert_eq!(provenance.regenerated_from_diff_id.as_deref(), Some("d1"));
        assert_eq!(provenance.regenerated_from_hunk_index, Some(2));
    }

    #[test]
    fn plan_mode_uses_dedicated_designer_pipeline() {
        let stages = crate::agent::multi_agent::plan_pipeline();

        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].role, AgentRole::Designer);
        assert_eq!(stages[1].role, AgentRole::Reviewer);
    }
}

/// Get the current steps.
#[tauri::command]
pub async fn get_agent_steps(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<TaskStep>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.steps.clone())
}

/// Get the current diffs.
#[tauri::command]
pub async fn get_agent_diffs(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<FileDiff>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.diffs.clone())
}

/// 读回撤不回的外部动作。
///
/// 和 `get_agent_diffs` 同一个位置：前端刷新后还能把"这次运行动了外面什么"拿回来，
/// 而不是只靠那条可能没人接收的事件。
#[tauri::command]
pub async fn get_agent_external_actions(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<crate::agent::orchestrator::ExternalActionRecord>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.external_actions.clone())
}

/// 忘掉更早会话留下的外部动作记录，返回忘掉了几条。
///
/// 只对更早的那些生效：让用户能抹掉手上这次运行刚做的事，等于把这份记录变成可以事后否认
/// 的东西。磁盘上留一条墓碑说明少了几条 —— 一份说不清自己被剪过的审计文件，和一份被人
/// 删过的审计文件在事后看起来一样。
///
/// 锁分两次拿，中间放文件 IO：这条不是运行路径上的，但一次磁盘读写不该把其他命令堵在
/// 后面。
#[tauri::command]
pub async fn forget_earlier_external_actions(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<usize, String> {
    let keep_ids: Vec<String> = {
        let orch = agent_state.orchestrator.lock().await;
        orch.external_actions
            .iter()
            .filter(|action| !action.restored)
            .map(|action| action.id.clone())
            .collect()
    };
    let forgotten = crate::agent::external_log::forget_earlier_sessions(&keep_ids)?;
    if forgotten > 0 {
        agent_state
            .orchestrator
            .lock()
            .await
            .forget_restored_external_actions();
    }
    Ok(forgotten)
}

#[tauri::command]
pub async fn get_agent_sdd_artifacts(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<SddArtifact>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.sdd_artifacts.clone())
}

#[tauri::command]
pub async fn save_sdd_artifact(
    request: SaveSddArtifactRequest,
) -> Result<SavedSddArtifactResponse, String> {
    if !executor::is_safe_slug(&request.artifact.slug) {
        return Err(format!("Invalid SDD slug: {}", request.artifact.slug));
    }
    let relative = format!("docs/design/{}.md", request.artifact.slug);
    let path = workspace::resolve_for_write(&relative)?;
    if path.exists() && !request.overwrite {
        return Err(format!("SDD already exists: {}", relative));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Create SDD directory: {}", e))?;
    }
    std::fs::write(&path, &request.artifact.markdown)
        .map_err(|e| format!("Write SDD artifact: {}", e))?;
    Ok(SavedSddArtifactResponse {
        path: path.to_string_lossy().to_string(),
        artifact: request.artifact,
    })
}

/// Get LLM configuration with the API key masked.
#[derive(Debug, Serialize)]
pub struct LlmConfigResponse {
    pub endpoint: String,
    pub api_key_masked: String,
    pub model: String,
    pub context_compression: String,
    pub profiles: Vec<LlmProfileResponse>,
    pub active_profile_id: String,
}

#[tauri::command]
pub async fn get_llm_config(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmConfigResponse, String> {
    let config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    let active = config
        .profiles
        .iter()
        .find(|profile| profile.id == config.active_profile_id)
        .or_else(|| config.profiles.first())
        .ok_or_else(|| "LLM config not set".to_string())?;
    Ok(LlmConfigResponse {
        endpoint: active.endpoint.clone(),
        api_key_masked: active.masked_api_key(),
        model: active.model.clone(),
        context_compression: agent_state
            .context_compression
            .lock()
            .map_err(|e| e.to_string())?
            .to_string(),
        profiles: config
            .profiles
            .iter()
            .map(|profile| profile.to_response())
            .collect(),
        active_profile_id: config.active_profile_id.clone(),
    })
}

#[tauri::command]
pub async fn save_llm_profile(
    request: SaveLlmProfileRequest,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmProfilesResponse, String> {
    let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    llm_profiles::save_profile(&mut config, request)
}

/// 读出当前（或指定）profile 的明文密钥，供设置面板的"显示"按钮使用。
#[tauri::command]
pub async fn reveal_llm_api_key(
    profile_id: Option<String>,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    llm_profiles::reveal_api_key(&config, profile_id.as_deref())
}

#[tauri::command]
pub async fn set_active_llm_profile(
    profile_id: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmProfilesResponse, String> {
    let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    llm_profiles::set_active_profile(&mut config, profile_id)
}

#[tauri::command]
pub async fn delete_llm_profile(
    profile_id: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<LlmProfilesResponse, String> {
    let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    llm_profiles::delete_profile(&mut config, profile_id)
}

#[tauri::command]
pub async fn set_context_compression(
    mode: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let parsed = ContextCompressionMode::from_str(&mode)?;
    {
        let mut current = agent_state
            .context_compression
            .lock()
            .map_err(|e| e.to_string())?;
        *current = parsed.clone();
    }
    let mut config = agent_state.llm_profiles.lock().map_err(|e| e.to_string())?;
    llm_profiles::set_context_compression_mode(&mut config, parsed.clone());
    Ok(parsed.to_string())
}

/// Save the workspace path to disk.
///
/// 顺带把外部动作记录换成新工作区那一份：磁盘上是按工作区归档的，而内存里这份此前只在
/// 启动时填过一次 —— 切过工作区之后屏幕上（和 Changes 角标上）还是上一个项目的动作，
/// 而新记录已经归到新项目名下了。
#[tauri::command]
pub async fn save_workspace_path(
    path: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<(), String> {
    let resolved = std::path::PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| format!("Workspace does not exist or is not accessible: {}", e))?;
    if !resolved.is_dir() {
        return Err(format!("Workspace is not a directory: {}", path));
    }
    workspace::save_workspace_path(&resolved.to_string_lossy())?;
    let restored = crate::agent::external_log::load_for_current_workspace();
    agent_state
        .orchestrator
        .lock()
        .await
        .rescope_external_actions(restored);
    Ok(())
}

/// Load the last saved workspace path from disk.
#[tauri::command]
pub fn get_workspace_path() -> Result<Option<String>, String> {
    workspace::load_workspace_path()
}

/// 没有任何任务描述可用时放进提示词的占位串。
///
/// 刻意是一句认得出来的话而不是空串，也因此**不能**被当成一轮真实历史存起来：
/// 存下去之后它就成了下一次的"原始任务描述"，一句诊断用的话被洗成了真实任务。
/// 见 `repair_workspace`。
pub const ORIGINAL_PROMPT_UNKNOWN: &str = "(original task not recorded)";

/// 修复/验证提示里那句"用户原本要什么"。
///
/// `verify_workspace`、`repair_workspace`、`agent_repair_prompt` 三个命令都要这个值，
/// 以前各写一份同样的 `match`。三条规则，都有测试钉住：
///   * 请求里给了就用请求的；
///   * **空白串按"没给"处理** —— 前端输入框留空送来的是 `Some("")`，而一个空的原始
///     任务描述会让模型只看到一堆报错，不知道要修成什么样；
///   * 连对话记录都没有时返回显式占位符，不是空串。提示里出现
///     "(original task not recorded)" 是可诊断的，一段空白不是。
fn resolve_original_prompt(requested: Option<String>, last_turn: Option<String>) -> String {
    match requested {
        Some(prompt) if !prompt.trim().is_empty() => prompt,
        _ => last_turn.unwrap_or_else(|| ORIGINAL_PROMPT_UNKNOWN.to_string()),
    }
}

/// 应用 / 撤销类操作在 action log 里的级别。
///
/// 只有一条规则：**有任何一条失败就不能记成 success**。这个产品的前提是用户能看清
/// Agent 到底做了什么，一次部分失败的应用被记成成功，用户就会以为改动全都落盘了 ——
/// 而实际上有文件没改到。四个命令（`apply_diffs` / `apply_diff` / `apply_diff_hunk` /
/// `undo_last_apply`）以前各写一份同样的三元表达式。
fn apply_log_level(failed: usize) -> &'static str {
    if failed == 0 {
        "success"
    } else {
        "warn"
    }
}

/// Test LLM connectivity with a small request.
#[tauri::command]
pub async fn test_llm_connection(
    agent_state: State<'_, AgentGlobalState>,
    profile_id: Option<String>,
) -> Result<String, String> {
    let (llm, _usage_meter) = agent_state.get_llm_client(profile_id.as_deref(), None)?;
    // 连通性探测有自己的取消开关。以前它清的是全局那个，于是"测试连接"这个
    // 无害动作会把一次正在跑的运行**取消解除**。
    let cancel_flag = std::sync::Arc::new(AtomicBool::new(false));

    let messages = vec![crate::services::llm_client::ChatMessage::user("Hi")];

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(4);
    let handle = tokio::spawn(async move {
        let mut full = String::new();
        while let Some(tok) = rx.recv().await {
            full.push_str(&tok);
        }
        full
    });

    match llm.stream_chat(messages, cancel_flag, tx).await {
        Ok(response) => {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let full = handle.await.unwrap_or(response);
            let preview: String = full.chars().take(120).collect();
            Ok(format!("OK - {}", preview))
        }
        // 到达了供应商、但它没给内容，不算"连不上"：端点、key、模型名都是对的，把它报成
        // 连接失败会让用户去查网络和密钥，而真正要改的是输出上限（推理模型上很常见）。
        Err(e) if e.contains("no message content") => Err(format!(
            "Reached the provider, but it returned no answer: {}",
            e
        )),
        Err(e) => Err(format!("Connection failed: {}", e)),
    }
}

/// Set the current Agent role.
#[tauri::command]
pub async fn set_active_role(
    role: String,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<String, String> {
    let parsed = match role.as_str() {
        "architect" => AgentRole::Architect,
        "designer" => AgentRole::Designer,
        "coder" => AgentRole::Coder,
        "tester" => AgentRole::Tester,
        "reviewer" => AgentRole::Reviewer,
        _ => return Err(format!("Invalid role: {}", role)),
    };
    let mut active = agent_state.active_role.lock().map_err(|e| e.to_string())?;
    *active = parsed;
    Ok(parsed.to_string().to_string())
}

/// Get the current Agent role.
#[tauri::command]
pub async fn get_active_role(agent_state: State<'_, AgentGlobalState>) -> Result<String, String> {
    let active = agent_state.active_role.lock().map_err(|e| e.to_string())?;
    Ok(active.to_string().to_string())
}

/// Get the current pipeline.
#[tauri::command]
pub async fn get_pipeline(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<PipelineStage>, String> {
    let stages = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?;
    Ok(stages.clone())
}

/// Update the pipeline.
#[tauri::command]
pub async fn update_pipeline(
    stages: Vec<PipelineStage>,
    agent_state: State<'_, AgentGlobalState>,
) -> Result<(), String> {
    let mut pipe = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?;
    *pipe = stages;
    Ok(())
}

/// Reset the pipeline to defaults.
#[tauri::command]
pub async fn reset_pipeline(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<PipelineStage>, String> {
    let mut pipe = agent_state
        .pipeline_stages
        .lock()
        .map_err(|e| e.to_string())?;
    *pipe = default_pipeline();
    Ok(pipe.clone())
}

/// 后端此刻真正会喂给模型的那几轮对话。
///
/// 界面上的消息流和这个列表是两回事：消息流不设上限也不持久化，而这里只留末尾
/// 若干轮、每轮都截断过，并且只在一次 `send_agent_prompt` **成功**时才记一轮。
/// 想让用户管理上下文，就必须让他看见真正的那一份，而不是看起来像的那一份。
#[tauri::command]
pub async fn get_agent_conversation(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<Vec<crate::agent::orchestrator::ConversationTurn>, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(orch.conversation.clone())
}

/// 项目记忆（`AGENTS.md`）此刻的状态，外加让 Agent 起草它的那句提示词。
///
/// 不需要 orchestrator：这只是读一个文件。状态和提示词一次取回，因为界面上它们是同一块 ——
/// "这个项目没有项目记忆"和"它有但尾部没发出去"都指向同一个动作。
///
/// 起草不走新路径：前端把这句提示词当普通提问发出去，Agent 用平常的写文件工具落地，
/// 改动照样经过审查区和撤销栈。这个功能的全部就是那段文字。
#[tauri::command]
pub async fn get_project_memory(
) -> Result<crate::services::project_memory::ProjectMemoryInfo, String> {
    crate::services::project_memory::project_memory_info()
}

/// 撤销某一轮对话改的文件。
///
/// 和 `undo_last_apply` 的区别是按**轮**算账：一轮里可能落盘好几次，而用户记得的是
/// "我让它做的那件事"。中间层不允许抽走（撤销栈严格后进先出），被挡住时后端会说清原因，
/// 这里把那句话原样传上去 —— 前端不兜、不改写。
#[tauri::command]
pub async fn revert_turn_changes(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
    turn_id: String,
) -> Result<crate::agent::orchestrator::UndoResult, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let result = orch.revert_turn_changes(&turn_id)?;
    let summary = format!(
        "Reverted {} file(s) from {}{}",
        result.restored.len(),
        turn_id,
        if result.failed.is_empty() {
            String::new()
        } else {
            format!(", {} could not be restored", result.failed.len())
        }
    );
    let details = if result.failed.is_empty() {
        result.restored.join("\n")
    } else {
        format!(
            "{}\nCould not restore: {}",
            result.restored.join("\n"),
            result.failed.join(", ")
        )
    };
    let _ = app_handle.emit(
        "agent-diff-ready",
        serde_json::to_value(&orch.diffs).unwrap_or_default(),
    );
    // 撤销可用性跟在这个事件的 payload 里，漏掉这一处撤销按钮会静默停在旧值
    let _ = app_handle.emit("agent-state-changed", orch.state_payload());
    // 等级按失败数走同一个 helper：部分失败记成 success 会让日志比现实乐观
    orch.emit_review_action_log(
        &app_handle,
        apply_log_level(result.failed.len()),
        "turn_revert",
        &summary,
        &details,
    );
    Ok(result)
}

/// 从指定的那一轮起把上下文切掉，返回剩下的几轮。
///
/// 顺带写一条 action log：这是一次用户主动的状态变更，而这个产品的前提是每一次
/// 变更都看得见。返回剩余列表而不是让界面再查一次 —— 中间多一次往返就多一个
/// "界面显示的和后端实际的不一致"的窗口。
#[tauri::command]
pub async fn truncate_agent_conversation(
    app_handle: AppHandle,
    agent_state: State<'_, AgentGlobalState>,
    turn_id: String,
) -> Result<Vec<crate::agent::orchestrator::ConversationTurn>, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let dropped = orch.truncate_conversation_from(&turn_id)?;
    orch.emit_review_action_log(
        &app_handle,
        "info",
        "context_truncate",
        &format!(
            "Dropped {} turn{} from the Agent context",
            dropped,
            if dropped == 1 { "" } else { "s" }
        ),
        &format!(
            "Cut from {}. Remaining turns: {}.",
            turn_id,
            orch.conversation.len()
        ),
    );
    Ok(orch.conversation.clone())
}

/// 开始一个新会话：清空对话历史、换一个会话 id。
///
/// 不清的话上一件事的摘要会继续被喂进新任务的上下文，既浪费预算也会误导模型。清空**同时**
/// 换会话 id：只清不换会让磁盘上那个会话的内容被悄悄换掉，用户点回历史看到的不是他记得的
/// 那一次。旧会话已经在磁盘上，回得去。
#[tauri::command]
pub async fn start_new_agent_session(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<AgentSessionList, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    if orch.run_in_flight() {
        return Err(RUN_IN_FLIGHT_SESSION_SWITCH.to_string());
    }
    orch.start_new_session();
    Ok(session_list(&orch))
}

/// 运行途中换会话会把这一轮历史记到别的会话里，所以每个入口共用同一句拒绝。
const RUN_IN_FLIGHT_SESSION_SWITCH: &str =
    "A run is still in flight. Stop it first, then switch sessions.";

/// 历史会话列表里的一行。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSummary {
    pub id: String,
    pub title: String,
    pub updated_at: u64,
    pub turn_count: usize,
    /// 最后一轮的结果。列表里给一行就够用来认出"是不是这次"。
    pub last_outcome: String,
}

/// `list_agent_sessions` 的返回值。
///
/// 带上 `warning`：查历史的地方正是该说"历史此刻写不进去"的地方 —— 那件事在界面上没有任何
/// 其他症状，用户会在下一次打开时才发现什么都没有。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionList {
    pub active_id: String,
    pub sessions: Vec<AgentSessionSummary>,
    pub warning: Option<String>,
    /// 这个环境此刻到底会不会存会话。
    ///
    /// 没打开过工作区时一条都存不下来（会话按工作区分组），而那时列表空着和"还没聊过"长得
    /// 一模一样。界面要照这个标志说清是哪一种，不然用户会以为自己刚才那一问被记住了。
    pub sessions_are_saved: bool,
}

/// 恢复一个历史会话之后界面需要知道的东西。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionDetail {
    pub id: String,
    pub title: String,
    pub turns: Vec<crate::agent::orchestrator::ConversationTurn>,
}

/// 磁盘上的列表 + 后端此刻的会话 id。
///
/// 三个命令（列、新建、删除）都返回同一份形状，所以只有一处拼装：各自拼一遍的话，"哪个是
/// 当前会话"在不同入口回来的答案可以不一样，而界面的高亮就是照它画的。
///
/// 读盘在持锁期间同步发生。没有跨 `await` 持锁，所以架构规则没有被破；代价是别的命令这段
/// 时间要排队等一次文件读，而这个文件最多 50 条会话。
fn session_list(orch: &crate::agent::orchestrator::AgentOrchestrator) -> AgentSessionList {
    let listed = crate::agent::session_store::list_for_current_workspace();
    let sessions = listed
        .sessions
        .into_iter()
        .map(|session| AgentSessionSummary {
            id: session.id,
            title: session.title,
            updated_at: session.updated_at,
            turn_count: session.turns.len(),
            last_outcome: session
                .turns
                .last()
                .map(|turn| turn.outcome.clone())
                .unwrap_or_default(),
        })
        .collect();
    AgentSessionList {
        active_id: orch.session_id.clone(),
        sessions,
        // 写失败和读失败都要说，而且写失败更急：它意味着**此刻**这一轮存不下来
        warning: orch.session_persist_error.clone().or(listed.warning),
        sessions_are_saved: crate::services::workspace::current_workspace_key().is_some(),
    }
}

/// 当前工作区的历史会话，最近更新的在前。
#[tauri::command]
pub async fn list_agent_sessions(
    agent_state: State<'_, AgentGlobalState>,
) -> Result<AgentSessionList, String> {
    let orch = agent_state.orchestrator.lock().await;
    Ok(session_list(&orch))
}

/// 回到一个历史会话：把那几轮对话装回上下文。
///
/// 只恢复上下文，不恢复 steps / diffs —— 见 `AgentOrchestrator::resume_session`。界面必须照
/// 这个事实措辞，否则用户会以为审查区里那些改动也一起回来了。
#[tauri::command]
pub async fn resume_agent_session(
    agent_state: State<'_, AgentGlobalState>,
    session_id: String,
) -> Result<AgentSessionDetail, String> {
    // "读不出来"和"没有这个会话"要分开说：把前者说成后者，用户会以为是自己删过它
    let stored = crate::agent::session_store::find(&session_id)
        .map_err(|reason| {
            format!(
                "The session history file could not be read ({}), so that session cannot be \
                 opened.",
                reason
            )
        })?
        .ok_or_else(|| format!("That session is no longer on disk ({}).", session_id))?;
    let title = stored.title.clone();
    let mut orch = agent_state.orchestrator.lock().await;
    if orch.run_in_flight() {
        return Err(RUN_IN_FLIGHT_SESSION_SWITCH.to_string());
    }
    orch.resume_session(stored);
    Ok(AgentSessionDetail {
        id: orch.session_id.clone(),
        title,
        turns: orch.conversation.clone(),
    })
}

/// 给一个任务改名，返回改完之后的列表。
///
/// 两条路，因为要改的东西不一样：改**当前**会话必须同时改内存里那份标题，否则下一轮对话会把
/// 旧标题原样写回去，表现为"改了又变回去"；改别的会话只需要动磁盘那一行。
///
/// 不检查是否在运行中：改名不动上下文，而一次长跑中间想把这个任务标清楚正是最自然的时刻。
#[tauri::command]
pub async fn rename_agent_session(
    agent_state: State<'_, AgentGlobalState>,
    session_id: String,
    title: String,
) -> Result<AgentSessionList, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    if orch.session_id == session_id {
        orch.rename_session(&title)?;
        return Ok(session_list(&orch));
    }
    let normalized = crate::agent::orchestrator::normalized_session_title(&title)?;
    if !crate::agent::session_store::rename(&session_id, &normalized)? {
        return Err(format!("That task is no longer on disk ({}).", session_id));
    }
    Ok(session_list(&orch))
}

/// 从一个任务分叉出一个新任务：同样的上下文，新的 id 和标题。
///
/// 和"回到那个任务"的区别是接下来往哪里写：恢复会把后续每一轮都写进原来那次记录，而分叉
/// 留着原记录不动 —— 用户想"用同一份上下文试另一条路"时要的正是后者。
///
/// 运行中拒绝，理由同恢复：换会话会让正在跑的那一轮把结果记到别人头上。
#[tauri::command]
pub async fn fork_agent_session(
    agent_state: State<'_, AgentGlobalState>,
    session_id: String,
) -> Result<AgentSessionDetail, String> {
    let stored = crate::agent::session_store::find(&session_id)
        .map_err(|reason| {
            format!(
                "The session history file could not be read ({}), so that task cannot be forked.",
                reason
            )
        })?
        .ok_or_else(|| format!("That task is no longer on disk ({}).", session_id))?;
    let mut orch = agent_state.orchestrator.lock().await;
    if orch.run_in_flight() {
        return Err(RUN_IN_FLIGHT_SESSION_SWITCH.to_string());
    }
    orch.fork_session(stored)?;
    Ok(AgentSessionDetail {
        id: orch.session_id.clone(),
        title: orch
            .session_snapshot()
            .map(|snapshot| snapshot.title)
            .unwrap_or_default(),
        turns: orch.conversation.clone(),
    })
}

/// 删掉一个历史会话，返回删完之后的列表。
///
/// 先删磁盘再动内存：反过来的话，删除失败（文件坏了、只读）会留下"上下文已经清空、会话
/// 却还在磁盘上"的状态 —— 界面收到报错、以为什么都没发生，而模型此刻已经看不到那几轮了。
///
/// 删的是**当前**这个会话时要顺带换掉 id：不换的话下一轮对话会把刚删掉的那一行原样写回来，
/// 表现为"删了没反应"。
#[tauri::command]
pub async fn delete_agent_session(
    agent_state: State<'_, AgentGlobalState>,
    session_id: String,
) -> Result<AgentSessionList, String> {
    let mut orch = agent_state.orchestrator.lock().await;
    let deleting_current = orch.session_id == session_id;
    if deleting_current && orch.run_in_flight() {
        return Err(RUN_IN_FLIGHT_SESSION_SWITCH.to_string());
    }
    crate::agent::session_store::remove(&session_id)?;
    if deleting_current {
        orch.start_new_session();
    }
    Ok(session_list(&orch))
}
