use crate::agent::events::RunEvents;
use crate::agent::executor;
use crate::agent::multi_agent::{
    default_pipeline, mark_pipeline_stage, plan_pipeline, reset_pipeline_status, AgentRole,
    PipelineStage,
};
use crate::agent::planner;
use crate::agent::state_machine::{
    AgentMode, AgentStateManager, DiffHunkProvenance, DiffProvenance, IdeMode, SddArtifact,
    TaskStep,
};
use crate::services::context::{
    estimated_input_tokens_from_budget, AgentContext, ContextBudget, ContextBuildOptions,
    ContextCompressionMode, ContextSourceOptions,
};
use crate::services::llm_client::LlmClient;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
// 这个文件里已经没有 Tauri 类型了：发事件走 `RunEvents`，所以 orchestrator
// 的流水线逻辑可以在没有桌面运行时的情况下被测试。
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize)]
pub struct ActionLogEntry {
    pub id: String,
    pub timestamp: String,
    pub level: String,
    pub phase: String,
    pub role: Option<String>,
    pub stage: Option<String>,
    pub summary: String,
    pub details: String,
    #[serde(rename = "contextSummary")]
    pub context_summary: Option<String>,
    #[serde(rename = "diffSummary")]
    pub diff_summary: Option<String>,
}

/// 有界修复循环的结果。
///
/// `stop` 说明为什么停：检查通过、预算耗尽、diff 落不了盘、没开启。四种情况
/// 对调用方的意义不同，所以不压缩成一个 bool。
#[derive(Debug)]
pub struct RepairLoopOutcome {
    /// 实际跑完的修复轮数
    pub iterations: u8,
    pub stop: crate::services::verification::RepairStop,
    /// 最后一次检查是否仍然失败
    pub checks_failed: bool,
    /// 最后一次检查的完整结果，供调用方展示或落成 artifact
    pub results: Vec<crate::services::project_tasks::RunProjectTaskResult>,
}

/// Agent orchestrator - main flow controller.
pub struct AgentOrchestrator {
    pub state_mgr: AgentStateManager,
    pub mode: AgentMode,
    pub ide_mode: IdeMode,
    pub steps: Vec<TaskStep>,
    pub diffs: Vec<crate::agent::state_machine::FileDiff>,
    pub sdd_artifacts: Vec<SddArtifact>,
    pub current_run_id: Option<String>,
    pub last_run_id: Option<String>,
    pub paused_run: Option<PausedPipelineRun>,
    /// 外部工具执行器（MCP）。None 表示本次运行不暴露外部工具。
    pub tool_invoker: Option<Arc<dyn crate::agent::executor::ToolInvoker>>,
    /// 本次运行使用的 MCP 放行策略。
    ///
    /// 记在这里是为了让 `continue_agent_pipeline` 能按同一策略重建工具面：
    /// 工具定义（进请求体）和执行器（跑调用）必须一起装，只装一半会让模型
    /// 看到工具却没人执行它的调用。
    pub tool_policy: crate::services::mcp::McpToolPolicy,
    /// 本次运行内置工作区工具的授权范围（命令执行清单）。
    /// 和 `tool_policy` 同理：续跑时要按原样重建工具面。
    pub tool_permissions: crate::agent::workspace_tools::WorkspaceToolPermissions,
    /// Auto 模式自动应用时是否允许创建新文件。
    ///
    /// 保守默认 false：请求没带这个权限时，新建文件的 diff 留给人工审查，
    /// 而不是被静默写盘。编辑已有文件不受影响。
    pub allow_file_create: bool,
    /// 本次运行的 token 记账器。
    ///
    /// 存在 orchestrator 上而不是只存在命令的局部变量里，是为了让 `continue_agent_pipeline`
    /// 恢复暂停的运行时能接着用同一个额度：否则续跑会重新从 0 记账，配置的
    /// 单次运行上限只要中途暂停一次就形同虚设。
    pub run_usage: Option<Arc<crate::services::llm_client::RunUsageMeter>>,
    /// 本会话里已经完成的几轮对话，最新的在最后。
    ///
    /// 没有它的话每次 prompt 都是冷启动 —— 跟进一句"再处理下错误分支"读不到
    /// 上一轮做了什么。只保留末尾若干轮并且每条都截断：这里要的是"上次干了啥"
    /// 的线索，不是完整逐字记录，后者会把上下文预算吃光。
    pub conversation: Vec<ConversationTurn>,
    /// 已应用批次的撤销栈，最新的在最后。
    ///
    /// diff 应用之前是单向的：一旦落盘就只能靠用户自己 git。审查界面能拒绝
    /// 还没应用的改动，却对已经应用的无能为力 —— 而"应用了才发现不对"恰恰是
    /// 最需要退路的时刻。
    undo_stack: Vec<ApplyCheckpoint>,
    /// 当前持有执行权的运行：凭据编号 + 它的存活凭证。`None` 表示空闲。
    ///
    /// 存 `Weak` 而不是 `bool`：`RunLease` 一旦被丢弃（正常收尾、提前返回、panic），
    /// 这个 `Weak` 就升不上来，执行权自动可回收。见 `try_begin_run`。
    active_claim: Option<(u64, std::sync::Weak<()>)>,
    /// 当前运行的取消开关。每次运行一个新的 Arc —— 见 `RunLease`。
    active_cancel: Option<Arc<AtomicBool>>,
    /// 同一个开关的对外句柄，让 Stop 不必先拿这把大锁。见 `CancelRegistry`。
    cancel_registry: CancelRegistry,
    /// 下一个要发的凭据编号。单调递增，只在进程内有意义。
    next_claim: u64,
}

/// 一次运行的执行权凭据。
///
/// 存在的理由是**只有持有者能释放**。第一版用一个 `bool`，于是先结束的那个运行会
/// 把还在跑的那个的执行权一起放掉 —— 而"先结束的不是先开始的"在这里是常态：
/// `stop_agent` 不等运行排空就返回，被停掉的那次可能还卡在一个不可中断的
/// `workspace_run_command` 里几分钟。它醒来收尾时，如果能无条件清标志，
/// 就等于给第三个 prompt 开了门，那正是这个机制要挡住的事。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunClaim(u64);

/// 一次运行的执行权 + 它自己的取消开关。
///
/// 取消开关**每次运行一个新的**，而不是全局共享一个。共享的那版有五处
/// `store(false)`：每个入口在启动前都要"先清掉上次的取消状态"，于是一个新 prompt
/// 会把还在排空的旧运行**取消解除** —— 用户点了 Stop，界面立刻变空闲，然后他接着
/// 发下一个问题，那个已经被停掉的运行就继续调模型、继续花钱。
///
/// 一次运行一个 Arc 之后这件事在结构上就不可能了：旧运行手里那个开关被置 true
/// 之后没有任何代码会再碰它，因为谁也拿不到它了。
#[must_use = "持有它才代表持有执行权；丢掉它等于放弃这次运行"]
pub struct RunLease {
    pub claim: RunClaim,
    pub cancel: Arc<AtomicBool>,
    /// 存活凭证。orchestrator 那边只留一个 `Weak`，所以**它一被丢弃，执行权就自动
    /// 可回收**。
    ///
    /// 这一层是拿来防我自己的：执行权的正确释放原本全靠调用方记得在每条退出路径上
    /// 调 `finish_run`，而这个会话里我已经在相邻的两个改动里各漏过一次同类的生命周期
    /// 管理。漏了的后果是所有后续运行被永久拒绝 —— 一个只能靠重启或 Stop 解开的死结。
    /// 现在漏掉最多让回收晚一点，而不是让应用卡死。
    _alive: Arc<()>,
}

/// 当前运行取消开关的**发布处**，用一把独立的小锁保护。
///
/// 存在的理由只有一个：**取消这条路不能依赖被取消的工作正持有的那把锁。**
/// 第一版把开关只放在 orchestrator 里，于是 `stop_agent` 得先拿 orchestrator 锁
/// 才能拉开关 —— 而 `repair_workspace` 会跨 await 一直持着那把锁，Stop 就只能
/// 干等到修复自己结束。等它终于拿到锁时，那次运行已经把开关交回去了，
/// `abandon_run` 拉了个空。Stop 变成一个只重置界面的空动作，而 Auto 模式下
/// 修复循环还在往磁盘上写。
///
/// 写入方只有 `try_begin_run` / `finish_run` / `abandon_run`，所以它不是第二份
/// 事实来源，而是同一个 per-run 对象的一个句柄出口。
#[derive(Clone, Default)]
pub struct CancelRegistry(Arc<std::sync::Mutex<Option<Arc<AtomicBool>>>>);

impl CancelRegistry {
    fn publish(&self, cancel: Arc<AtomicBool>) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(cancel);
        }
    }

    fn clear(&self) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
    }

    /// 拉下当前运行的开关。没有运行在跑就什么都不做。
    ///
    /// 不需要 orchestrator 锁 —— 这正是这个类型存在的全部意义。
    pub fn cancel_active_run(&self) {
        if let Ok(slot) = self.0.lock() {
            if let Some(cancel) = slot.as_ref() {
                cancel.store(true, Ordering::SeqCst);
            }
        }
    }
}

/// 一次应用操作的回滚点
#[derive(Clone, Debug)]
pub struct ApplyCheckpoint {
    /// 触发这次应用的操作，用于告诉用户将要撤销什么
    pub label: String,
    snapshots: Vec<crate::agent::diff_apply::FileSnapshot>,
}

impl ApplyCheckpoint {
    pub fn files(&self) -> Vec<String> {
        self.snapshots
            .iter()
            .map(|snapshot| snapshot.file.clone())
            .collect()
    }
}

/// 撤销结果
#[derive(Clone, Debug, Serialize)]
pub struct UndoResult {
    pub label: String,
    pub restored: Vec<String>,
    pub failed: Vec<String>,
}

/// 保留的撤销层数
const MAX_UNDO_CHECKPOINTS: usize = 20;

/// 一轮已完成的对话：用户说了什么，以及那一轮的结果
#[derive(Clone, Debug, Serialize)]
pub struct ConversationTurn {
    pub prompt: String,
    pub outcome: String,
}

/// 保留的对话轮数
const MAX_CONVERSATION_TURNS: usize = 6;
/// 每轮 prompt / 结果各自的字符上限
const MAX_TURN_PROMPT_CHARS: usize = 400;
const MAX_TURN_OUTCOME_CHARS: usize = 300;

/// 一次 pipeline 运行的**本地**状态。
///
/// 刻意不放进 `AgentOrchestrator`：这些字段只属于这一次运行，而 orchestrator 是
/// 跨运行长期存在的共享状态。`PausedPipelineRun` 早就是这份状态的快照 —— 暂停要存
/// 的就是它，这说明边界本来就在这里。
///
/// 把它拿出来是锁粒度治理的第一步：只有当"运行本地状态"不再借用 orchestrator，
/// 驱动器才能在 `execute_stage` 的 await 期间放开锁。
#[derive(Debug, Clone)]
pub struct PipelineRun {
    pub prompt: String,
    pub ctx_str: String,
    pub context_summary: String,
    pub pipeline: Vec<PipelineStage>,
    pub transcript: Vec<crate::services::llm_client::ChatMessage>,
    pub ide_mode: IdeMode,
}

/// 取消时统一返回的错误串。
///
/// 命令层靠它把"用户主动停止"和真正的失败区分开，所以两边必须用同一个常量：
/// 各写一遍字面量的话，改动一处就会让取消被当成错误弹给用户。
pub const CANCELLED_ERROR: &str = "Agent task cancelled";

/// `prepare_stage` 的结论：这个阶段可以跑，还是运行已经在它前面停住。
///
/// `Ready` 刻意把 `execute_stage` 需要的一切都装成**自有值**（含克隆出的
/// `tool_invoker` Arc）。一旦它不再借用 orchestrator，调用方就能在 await
/// 期间放开锁 —— 这是锁粒度治理要的形状。
pub enum StagePlan {
    Ready {
        stage: PipelineStage,
        /// 按 **id** 而不是下标记住这个阶段对应的计划条目。
        ///
        /// 驱动器在 `execute_stage` 期间不持锁，`stop_agent` 可以在此期间清空
        /// `steps` —— 那时任何缓存下来的下标都会越界 panic。id 找不到就说明
        /// 这次运行已经被中止，收尾时什么都不该再落地。
        step_id: String,
        pending_diff_summary: String,
        tool_invoker: Option<Arc<dyn crate::agent::executor::ToolInvoker>>,
    },
    /// `pause_before` 命中：暂停快照已写好，状态已置 `WaitingUser`。
    /// 调用方必须就此结束本次运行。
    Paused,
}

/// 修复循环的本地状态，由驱动器持有 —— 和 `PipelineRun` 同一个道理。
///
/// 它不放进 orchestrator，因为这些值属于**这一次**修复：一次修复的检查结果和
/// 轮次计数对下一次毫无意义，存进共享状态只会多一份要记得清空的东西。
pub struct RepairRun {
    pub original_prompt: String,
    pub commands: Vec<String>,
    pub root: std::path::PathBuf,
    pub policy: crate::services::verification::RepairPolicy,
    /// 最近一次检查的完整结果
    pub results: Vec<crate::services::project_tasks::RunProjectTaskResult>,
    pub checks_failed: bool,
    pub completed: u8,
    pub apply_failed: bool,
}

/// `prepare_repair_iteration` 的结论：再修一轮，还是到此为止。
pub enum RepairPlan {
    Iterate {
        iteration: u8,
        step: TaskStep,
        prompt: String,
        tool_invoker: Option<Arc<dyn crate::agent::executor::ToolInvoker>>,
    },
    Done(RepairLoopOutcome),
}

#[derive(Debug, Clone)]
pub struct PausedPipelineRun {
    pub prompt: String,
    pub context: String,
    pub context_summary: String,
    /// 已完成阶段的真实消息线程（含工具调用与工具结果）。
    ///
    /// 以前这里是 `stage_outputs: Vec<String>`，只有扁平文本，续跑之后模型看不到
    /// 暂停前工具实际返回了什么。
    pub transcript: Vec<crate::services::llm_client::ChatMessage>,
    pub pipeline: Vec<PipelineStage>,
    pub stage_index: usize,
    pub ide_mode: IdeMode,
}

impl Default for AgentOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// 跑完一次完整的 Agent 流程：prompt -> 规划 -> 逐阶段执行 -> 产出 diff -> 等用户。
///
/// 拿 `&Mutex<AgentOrchestrator>` 而不是 `&mut AgentOrchestrator`：**由驱动器决定
/// 什么时候持锁**，这是整个改造的要点。以前命令层在整段运行期间持锁，于是
/// `stop_agent`、`apply_diffs`、状态查询全都要排在几分钟的模型调用后面 —— 界面
/// 上表现为按钮点了没反应。现在锁只在每个同步步骤内短暂持有，模型调用期间放开。
#[allow(clippy::too_many_arguments)]
pub async fn drive_run(
    orch: &tokio::sync::Mutex<AgentOrchestrator>,
    prompt: String,
    context: AgentContext,
    context_compression: ContextCompressionMode,
    context_budget: Option<ContextBudget>,
    context_sources: ContextSourceOptions,
    pipeline: Vec<PipelineStage>,
    ide_mode: IdeMode,
    cancel_flag: Arc<AtomicBool>,
    llm: &LlmClient,
    events: Arc<dyn RunEvents>,
) -> Result<(), String> {
    let mut run = orch.lock().await.begin_planning(
        prompt,
        &context,
        context_compression,
        context_budget,
        &context_sources,
        pipeline,
        ide_mode,
        events.as_ref(),
    );

    let (tx, mut rx) = mpsc::channel::<String>(32);
    let events_clone = events.clone();
    tokio::spawn(async move {
        while let Some(token) = rx.recv().await {
            events_clone.emit_json("agent-stream-token", serde_json::json!(token));
        }
    });

    let (steps, planner_response) =
        planner::plan_task(llm, &run.prompt, &run.ctx_str, cancel_flag.clone(), tx).await?;

    orch.lock().await.record_plan(
        &mut run,
        steps,
        &planner_response,
        &cancel_flag,
        events.as_ref(),
    )?;

    drive_pipeline(orch, run, 0, false, cancel_flag, llm, events).await
}

/// 从 `start_index` 起驱动流水线，每个阶段三次短持锁。
///
/// 每一次 `lock().await` 都是语句级临时借用，出了语句就释放；`execute_stage`
/// 的 await 期间没有任何锁在手。三个同步方法各自是一个临界区，它们的不变量
/// 写在各自的文档里。
#[allow(clippy::too_many_arguments)]
pub async fn drive_pipeline(
    orch: &tokio::sync::Mutex<AgentOrchestrator>,
    mut run: PipelineRun,
    start_index: usize,
    ignore_pause_once: bool,
    cancel_flag: Arc<AtomicBool>,
    llm: &LlmClient,
    events: Arc<dyn RunEvents>,
) -> Result<(), String> {
    orch.lock().await.ide_mode = run.ide_mode;

    for stage_index in start_index..run.pipeline.len() {
        let skip_pause = ignore_pause_once && stage_index == start_index;
        let plan =
            orch.lock()
                .await
                .prepare_stage(&mut run, stage_index, skip_pause, events.as_ref());
        let StagePlan::Ready {
            stage,
            step_id,
            pending_diff_summary,
            tool_invoker,
        } = plan
        else {
            // 暂停快照已经写好了，接着跑就会覆盖掉它
            return Ok(());
        };

        let (tx2, mut rx2) = mpsc::channel::<String>(32);
        let events_clone2 = events.clone();
        tokio::spawn(async move {
            while let Some(token) = rx2.recv().await {
                events_clone2.emit_json("agent-stream-token", serde_json::json!(token));
            }
        });

        let outcome = executor::execute_stage(
            llm,
            stage.role,
            &stage.name,
            &run.prompt,
            &run.ctx_str,
            &run.transcript,
            &pending_diff_summary,
            tool_invoker.as_deref(),
            cancel_flag.clone(),
            tx2,
        )
        .await;

        orch.lock().await.record_stage_outcome(
            &mut run,
            stage_index,
            &stage,
            &step_id,
            outcome,
            &cancel_flag,
            events.as_ref(),
        )?;

        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    }

    orch.lock().await.finish_pipeline(&run, events.as_ref())
}

/// 有界修复循环：跑检查 → 失败就让模型改 → 落盘 → 再跑检查。
///
/// CLI 早就有这套（`--max-iterations`），桌面端一直只有单轮：`Verify All` /
/// `Fix with Agent` 各自跑一次，失败之后要人再点一遍。停止规则和 CLI 共用
/// `verification::RepairPolicy`，所以两个入口对"什么时候该放弃"的判断不会各自漂移。
///
/// 和 `drive_pipeline` 同一形状，而且是**同一个理由**：这个循环原来是 orchestrator
/// 上的一个 `&mut self` 方法，于是命令层要跨整段 await 持着锁。后果不止是 Stop 拉不到
/// 开关（那条已经靠 `CancelRegistry` 绕开），还有 `get_agent_state` 一起被堵 ——
/// 界面因此永远不知道后端在忙，`isAgentBusy` 保持 false，Send / Apply 仍可点，
/// 而执行权守卫那句拒绝信息恰好在它最该出现的碰撞里到不了用户眼前。
#[allow(clippy::too_many_arguments)]
pub async fn drive_repair(
    orch: &tokio::sync::Mutex<AgentOrchestrator>,
    original_prompt: String,
    commands: Vec<String>,
    policy: crate::services::verification::RepairPolicy,
    cancel: Arc<AtomicBool>,
    llm: &LlmClient,
    events: Arc<dyn RunEvents>,
) -> Result<RepairLoopOutcome, String> {
    use crate::services::verification::{failed_command_results, run_checks};

    let root = crate::services::workspace::workspace_root()?;
    let results = run_checks(commands.clone(), root.clone()).await;
    let mut run = RepairRun {
        original_prompt,
        commands,
        root,
        policy,
        checks_failed: !failed_command_results(&results).is_empty(),
        results,
        completed: 0,
        apply_failed: false,
    };

    loop {
        let plan = orch
            .lock()
            .await
            .prepare_repair_iteration(&mut run, &cancel, events.as_ref())?;
        let (iteration, step, prompt, tool_invoker) = match plan {
            RepairPlan::Done(outcome) => return Ok(outcome),
            RepairPlan::Iterate {
                iteration,
                step,
                prompt,
                tool_invoker,
            } => (iteration, step, prompt, tool_invoker),
        };

        let (tx, mut rx) = mpsc::channel::<String>(32);
        let events_clone = events.clone();
        tokio::spawn(async move {
            while let Some(token) = rx.recv().await {
                events_clone.emit_json("agent-stream-token", serde_json::json!(token));
            }
        });
        let response = executor::execute_step(
            llm,
            &prompt,
            "",
            tool_invoker.as_deref(),
            cancel.clone(),
            tx,
        )
        .await?;

        let (applied_count, failed_count) = orch
            .lock()
            .await
            .record_repair_apply(&mut run, &step, &response);

        // 先克隆再赋值：`run.results = run_checks(run.commands.clone(), ..)` 会在
        // 同一个表达式里既读又写 `run`
        let commands = run.commands.clone();
        let root = run.root.clone();
        run.results = run_checks(commands, root).await;
        run.checks_failed = !failed_command_results(&run.results).is_empty();
        run.completed = iteration;

        orch.lock().await.record_repair_iteration(
            &run,
            iteration,
            applied_count,
            failed_count,
            events.as_ref(),
        );
    }
}

impl AgentOrchestrator {
    pub fn new() -> Self {
        Self {
            state_mgr: AgentStateManager::new(),
            mode: AgentMode::Suggest,
            ide_mode: IdeMode::Code,
            steps: Vec::new(),
            diffs: Vec::new(),
            sdd_artifacts: Vec::new(),
            current_run_id: None,
            last_run_id: None,
            paused_run: None,
            tool_invoker: None,
            tool_policy: crate::services::mcp::McpToolPolicy::AutoApprovedOnly,
            tool_permissions: crate::agent::workspace_tools::WorkspaceToolPermissions::read_only(),
            allow_file_create: false,
            run_usage: None,
            conversation: Vec::new(),
            undo_stack: Vec::new(),
            active_claim: None,
            active_cancel: None,
            cancel_registry: CancelRegistry::default(),
            next_claim: 0,
        }
    }

    /// 记一个回滚点。空快照不记：撤销一个什么都没写的操作会让栈顶失真。
    fn push_undo_checkpoint(
        &mut self,
        label: &str,
        snapshots: Vec<crate::agent::diff_apply::FileSnapshot>,
    ) {
        if snapshots.is_empty() {
            return;
        }
        self.undo_stack.push(ApplyCheckpoint {
            label: label.to_string(),
            snapshots,
        });
        if self.undo_stack.len() > MAX_UNDO_CHECKPOINTS {
            let excess = self.undo_stack.len() - MAX_UNDO_CHECKPOINTS;
            self.undo_stack.drain(..excess);
        }
    }

    /// 把 Agent 写入工具落下的改动登记成可审查、可撤销的 diff。
    ///
    /// 直接写盘会绕过审查区：文件变了，而 Diff 视图里什么都没有，用户失去了
    /// "Agent 到底改了什么"的可见性 —— 而这正是这个产品的核心价值。所以每次
    /// 写入事后都合成一张 `applied` 状态的 diff 卡片（original = 写前内容），
    /// 并压一个回滚点，让 `Undo Apply` 对工具写入同样有效。
    ///
    /// 同一文件被写多次时合并成一条：original 取**第一次**写之前的内容，
    /// updated 取**最后一次**写入的内容。撤销要回到"这次运行之前"，而不是
    /// 回到中间某一步。
    ///
    /// **锁不变量**：压回滚点（293）、挂 diff（294）、重刷 baseHash（297）必须在
    /// **一个**临界区里完成，而且上游 `permissions.take_writes()` 的排空也要算在
    /// 同一段里（`commands/agent.rs::publish_tool_writes`）。写入日志一旦排空就没了
    /// 第二次机会：中途被打断意味着文件已经在磁盘上，而审查区没有卡片、撤销栈没有
    /// 那一笔 —— 没有任何地方还能补回来。
    ///
    /// `stamp_base_hashes` 会重写**所有** diff 的 baseHash，所以 `diffs` 不能按
    /// 单条 diff 拆锁：一个正在校验 staleness 的 `apply_diff` 会读到写了一半的哈希。
    pub fn record_tool_writes(
        &mut self,
        writes: Vec<crate::agent::workspace_tools::AgentFileWrite>,
    ) -> Vec<crate::agent::state_machine::FileDiff> {
        use crate::agent::state_machine::{DiffHunk, DiffProvenance, FileDiff};

        if writes.is_empty() {
            return Vec::new();
        }

        // 按文件聚合，保持首次出现的顺序
        let mut order: Vec<String> = Vec::new();
        let mut merged: std::collections::HashMap<
            String,
            crate::agent::workspace_tools::AgentFileWrite,
        > = std::collections::HashMap::new();
        for write in writes {
            match merged.get_mut(&write.file) {
                Some(existing) => existing.updated = write.updated,
                None => {
                    order.push(write.file.clone());
                    merged.insert(write.file.clone(), write);
                }
            }
        }

        let mut snapshots = Vec::new();
        let mut created = Vec::new();
        for file in &order {
            let Some(write) = merged.remove(file) else {
                continue;
            };
            snapshots.push(crate::agent::diff_apply::FileSnapshot {
                file: write.file.clone(),
                path: write.path.clone(),
                previous: write.previous.clone(),
            });
            let is_new = write.previous.is_none();
            let diff = FileDiff {
                id: uuid::Uuid::new_v4().to_string(),
                file: write.file.clone(),
                base_hash: None,
                provenance: Some(DiffProvenance {
                    protocol: "workspace_tool".to_string(),
                    operation: if is_new { "create" } else { "edit" }.to_string(),
                    rationale: Some(
                        "Written directly by the Agent through workspace_write_file".to_string(),
                    ),
                    schema_version: None,
                    change_index: None,
                    source_role: None,
                    source_stage: Some("Tool Call".to_string()),
                    regenerated_from_diff_id: None,
                    regenerated_from_hunk_index: None,
                }),
                hunks: vec![DiffHunk {
                    old_start: 1,
                    old_lines: write
                        .previous
                        .as_deref()
                        .map(|previous| previous.lines().count() as u32)
                        .unwrap_or(0),
                    new_start: 1,
                    new_lines: write.updated.lines().count() as u32,
                    content: String::new(),
                    original: write.previous.clone().unwrap_or_default(),
                    updated: write.updated.clone(),
                    provenance: None,
                    // 已经落盘了，状态必须如实反映，否则用户会以为还能审查
                    status: Some("applied".to_string()),
                }],
                status: "applied".to_string(),
            };
            created.push(diff);
        }

        self.push_undo_checkpoint("Agent tool writes", snapshots);
        self.diffs.extend(created.clone());
        // 磁盘内容变了，其他还挂着的 diff 的 baseHash 要跟着刷新，
        // 否则它们会被误判 stale
        crate::agent::diff_apply::stamp_base_hashes(&mut self.diffs);
        self.refresh_review_state();
        created
    }

    /// 栈顶回滚点的描述，供界面显示"将要撤销什么"
    pub fn pending_undo(&self) -> Option<(String, Vec<String>)> {
        self.undo_stack
            .last()
            .map(|checkpoint| (checkpoint.label.clone(), checkpoint.files()))
    }

    /// 撤销最近一次应用：把文件恢复到那次应用之前。
    ///
    /// 恢复完成后把受影响的 diff 退回 `pending`，这样它们重新回到审查区，
    /// 而不是留在"已应用"却和磁盘不一致的状态。
    pub fn undo_last_apply(&mut self) -> Result<UndoResult, String> {
        let Some(checkpoint) = self.undo_stack.pop() else {
            return Err("Nothing to undo: no applied change is recorded".to_string());
        };
        let (restored, failed) = crate::agent::diff_apply::restore_snapshots(&checkpoint.snapshots);

        for diff in &mut self.diffs {
            if !restored.contains(&diff.file) {
                continue;
            }
            for hunk in &mut diff.hunks {
                if hunk.status.as_deref() == Some("applied") {
                    hunk.status = None;
                }
            }
            diff.status = status_from_hunks(&diff.hunks);
        }
        // 文件内容变回去了，baseHash 必须跟着变，否则重新应用会被误判 stale
        crate::agent::diff_apply::stamp_base_hashes(&mut self.diffs);
        self.refresh_review_state();

        Ok(UndoResult {
            label: checkpoint.label,
            restored,
            failed,
        })
    }

    /// 之前几轮的摘要，喂回下一次运行的上下文；没有历史时返回 None
    pub fn conversation_digest(&self) -> Option<String> {
        if self.conversation.is_empty() {
            return None;
        }
        let digest = self
            .conversation
            .iter()
            .enumerate()
            .map(|(index, turn)| {
                format!(
                    "{}. asked: {}\n   result: {}",
                    index + 1,
                    turn.prompt,
                    turn.outcome
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Some(digest)
    }

    /// 记一轮已完成的对话。结果由当前状态推导，命令层不需要拼摘要。
    pub fn record_conversation_turn(&mut self, prompt: &str) {
        let reviewable: Vec<&str> = self
            .diffs
            .iter()
            .filter(|diff| is_reviewable_diff_status(&diff.status))
            .map(|diff| diff.file.as_str())
            .collect();
        let applied = self
            .diffs
            .iter()
            .filter(|diff| diff.status == "applied")
            .count();

        let outcome = if reviewable.is_empty() && applied == 0 {
            "no file changes produced".to_string()
        } else {
            let mut parts = Vec::new();
            if applied > 0 {
                parts.push(format!("{} file(s) applied", applied));
            }
            if !reviewable.is_empty() {
                parts.push(format!("awaiting review: {}", reviewable.join(", ")));
            }
            parts.join("; ")
        };

        self.conversation.push(ConversationTurn {
            prompt: summarize_text(prompt.trim(), MAX_TURN_PROMPT_CHARS),
            outcome: summarize_text(&outcome, MAX_TURN_OUTCOME_CHARS),
        });
        // 只留末尾若干轮：早期的轮次对"接着上一句"没什么帮助，却一直占预算
        if self.conversation.len() > MAX_CONVERSATION_TURNS {
            let excess = self.conversation.len() - MAX_CONVERSATION_TURNS;
            self.conversation.drain(..excess);
        }
    }

    /// 开始新任务时清空对话历史
    pub fn clear_conversation(&mut self) {
        self.conversation.clear();
    }

    /// 换 run id。**不**授予执行权 —— 那是 `try_begin_run` 的事。
    ///
    /// 曾经在这里顺手置过"在跑"标志，结果任何调用它的入口都能绕过守卫拿到执行权。
    pub fn begin_run(&mut self, run_id: Option<String>) {
        self.current_run_id = run_id.clone();
        self.last_run_id = run_id;
    }

    /// 抢占本次运行的执行权，抢不到就拒绝。
    ///
    /// "同一时刻只有一个运行"以前是命令层整段持锁**顺带**保证的。锁收窄到每阶段
    /// 之后，两个 prompt 能真正并发进来，而它们共用 `steps`、`diffs` 和状态机 ——
    /// 后进来的那个 `record_plan` 会把前一个的计划整个换掉，前一个的阶段则往一份
    /// 已经不属于它的计划里写状态。所以把这条约束显式化，而不是继续依赖锁的副作用。
    ///
    /// 不看 `current_run_id`：run id 是调用方可选传的，缺省时它一直是 `None`，
    /// 拿它当"在跑"的判据会让守卫在最需要的时候失效。
    ///
    /// 返回的凭据必须原样传给 `finish_run`，取消开关则要一路传给流水线。
    ///
    /// 漏掉 `finish_run` 不会把应用锁死：`RunLease` 一被丢弃，这里存的 `Weak` 就
    /// 升不上来，下一次抢占会把这个已经没人持有的执行权直接回收。
    #[must_use = "拿到 lease 才算持有执行权"]
    pub fn try_begin_run(&mut self, run_id: Option<String>) -> Result<RunLease, String> {
        // 只有凭证还活着才算真的有人在跑。凭证已经没了说明上一个运行没走正常收尾
        // （提前返回或 panic），那把执行权回收掉，而不是让它永久占着。
        if let Some((_, alive)) = &self.active_claim {
            if alive.strong_count() > 0 {
                return Err(
                    "An Agent run is already in progress. Stop it before starting another."
                        .to_string(),
                );
            }
        }
        let claim = self.next_claim;
        self.next_claim = self.next_claim.wrapping_add(1);
        let alive = Arc::new(());
        self.active_claim = Some((claim, Arc::downgrade(&alive)));
        // 全新的开关，不是把上一个清零：上一个可能还被一个正在排空的运行握着
        let cancel = Arc::new(AtomicBool::new(false));
        self.active_cancel = Some(cancel.clone());
        self.cancel_registry.publish(cancel.clone());
        self.begin_run(run_id);
        Ok(RunLease {
            claim: RunClaim(claim),
            cancel,
            _alive: alive,
        })
    }

    /// 取消开关的对外句柄，交给命令层，让 Stop 不用先抢这把锁。
    pub fn cancel_registry(&self) -> CancelRegistry {
        self.cancel_registry.clone()
    }

    /// 开启一个新的用量记账周期（一次全新运行）
    pub fn start_usage_accounting(
        &mut self,
        meter: Arc<crate::services::llm_client::RunUsageMeter>,
    ) {
        self.run_usage = Some(meter);
    }

    /// 恢复暂停的运行时沿用的记账器；没有可沿用的就返回 None
    pub fn resumed_usage_meter(&self) -> Option<Arc<crate::services::llm_client::RunUsageMeter>> {
        self.run_usage.clone()
    }

    /// 交还执行权。**不是持有者就什么都不做。**
    ///
    /// 一个被 `stop_agent` 放弃、但还卡在不可中断工具调用里的旧运行，醒来后一定会
    /// 走到这里。那时执行权可能已经属于一个新运行 —— 让它无条件清掉，就等于给
    /// 第三个 prompt 开门。
    pub fn finish_run(&mut self, claim: RunClaim) {
        if self.active_claim.as_ref().map(|(id, _)| *id) != Some(claim.0) {
            return;
        }
        self.current_run_id = None;
        self.active_claim = None;
        self.active_cancel = None;
        self.cancel_registry.clear();
    }

    /// 用户主动放弃当前运行（Stop）：**先拉下取消开关**，再无条件释放执行权。
    ///
    /// 和 `finish_run` 分开是因为这里的语义相反 —— Stop 就是要抢回控制权，
    /// 包括从一个卡住的运行手里。代价是那个运行醒来时已经不是持有者，
    /// 而 `finish_run` 的持有者检查正好接住这一点。
    ///
    /// 开关是那次运行**自己的** Arc，所以置 true 之后不会有任何人再把它清零：
    /// 新运行拿到的是另一个全新的开关。这就是"取消被下一个 prompt 解除"这个
    /// bug 在结构上消失的地方。
    pub fn abandon_run(&mut self) {
        if let Some(cancel) = self.active_cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
        self.cancel_registry.clear();
        self.current_run_id = None;
        self.active_claim = None;
    }

    /// 规划阶段之前的全部状态变更，同步完成。
    ///
    /// 返回的 `PipelineRun` 还没有 transcript —— 那要等 planner 回话，见
    /// `record_plan`。中间的模型调用不该占着锁，所以这里就断开。
    ///
    /// **锁不变量**：整个函数必须在**一个**临界区里跑完。它把 `ide_mode`、状态机
    /// 和"本次运行用哪条流水线"一起定下来；前端拿到 pipeline 事件时状态必须已经
    /// 是 Thinking，否则界面会显示一条无人在跑的流水线。
    #[allow(clippy::too_many_arguments)]
    pub fn begin_planning(
        &mut self,
        prompt: String,
        context: &AgentContext,
        context_compression: ContextCompressionMode,
        context_budget: Option<ContextBudget>,
        context_sources: &ContextSourceOptions,
        pipeline: Vec<PipelineStage>,
        ide_mode: IdeMode,
        events: &dyn RunEvents,
    ) -> PipelineRun {
        use crate::agent::state_machine::AgentEvent;

        self.ide_mode = ide_mode;
        let _ = self
            .state_mgr
            .transition(&AgentEvent::UserPrompt(prompt.clone()));
        self.emit_state(events);

        let raw_ctx_str = context.to_prompt_context_with_mode(&context_compression);
        let ctx_str = context.to_prompt_context_with_options(&ContextBuildOptions::new(
            context_compression.clone(),
            context_budget.clone(),
        ));
        let context_summary = summarize_text(&ctx_str, 600);
        let budget_summary = format_context_budget_summary(
            context_budget.as_ref(),
            raw_ctx_str.len(),
            ctx_str.len(),
        );
        self.emit_action_log(
            events,
            "info",
            "prompt",
            None,
            None,
            "Agent prompt received",
            &format!(
                "Prompt:\n{}\n\nContext mode: {}\n{}\n{}",
                prompt,
                context_compression,
                budget_summary,
                format_context_sources(context_sources)
            ),
            Some(context_summary.clone()),
            None,
        );
        // 只在用户没有自定义流水线时才按请求形状裁剪。显式配置过阶段的人
        // 不该被悄悄改掉，那比多花点 token 更糟。
        let shape = crate::agent::task_shape::classify(&prompt);
        let trim_to_direct = ide_mode == IdeMode::Code
            && pipeline.is_empty()
            && shape == crate::agent::task_shape::TaskShape::Direct;
        let pipeline = if ide_mode == IdeMode::Plan {
            reset_pipeline_status(&plan_pipeline())
        } else if trim_to_direct {
            reset_pipeline_status(&crate::agent::multi_agent::direct_pipeline())
        } else if pipeline.is_empty() {
            reset_pipeline_status(&default_pipeline())
        } else {
            reset_pipeline_status(&pipeline)
        };
        if trim_to_direct {
            self.emit_action_log(
                events,
                "info",
                "pipeline_shape",
                None,
                None,
                "Single-file request: running the implement stage only",
                "Design, Test and Review were skipped because the request looks like a one-spot change. \
                 Configure the pipeline explicitly in Settings to always run every stage.",
                None,
                None,
            );
        }
        self.emit_pipeline(events, &pipeline);

        PipelineRun {
            prompt,
            ctx_str,
            context_summary,
            pipeline,
            transcript: Vec::new(),
            ide_mode,
        }
    }

    /// 收下 planner 的结果，同步完成。
    ///
    /// **锁不变量**：整个函数必须在**一个**临界区里跑完。`steps` 落地、状态翻到
    /// Planning、`agent-plan-ready` 事件必须一起发生 —— 前端收到计划事件后会照
    /// `steps` 渲染，读到只写了一半的组合就会画出空计划。
    pub fn record_plan(
        &mut self,
        run: &mut PipelineRun,
        steps: Vec<TaskStep>,
        planner_response: &str,
        cancel_flag: &Arc<AtomicBool>,
        events: &dyn RunEvents,
    ) -> Result<(), String> {
        use crate::agent::state_machine::AgentEvent;

        self.emit_action_log(
            events,
            "success",
            "planner",
            None,
            Some("Planner"),
            &format!(
                "Planner produced {} step{}",
                steps.len(),
                if steps.len() == 1 { "" } else { "s" }
            ),
            planner_response,
            Some(run.context_summary.clone()),
            None,
        );

        self.steps = steps;
        self.ensure_not_cancelled(cancel_flag, events)?;

        let _ = self
            .state_mgr
            .transition(&AgentEvent::PlanReady(self.steps.clone()));
        self.emit_state(events);
        events.emit_json(
            "agent-plan-ready",
            serde_json::to_value(&self.steps).unwrap_or_default(),
        );

        run.transcript
            .push(crate::services::llm_client::ChatMessage::assistant(
                format!("[Planner]\n{}", planner_response),
            ));
        Ok(())
    }

    /// 进入某个阶段前的全部状态变更，同步完成。
    ///
    /// **锁不变量**：整个函数必须在**一个**临界区里跑完。命中 `pause_before` 时它
    /// 要写暂停快照（`paused_run`）、把阶段标成 `paused`、置 `WaitingUser` 三件事 ——
    /// 三者之间被别的命令插入，会出现"快照已存但状态还在 running"或反之的窗口，
    /// 前端据此判断能否续跑，读到中间态就会给出错误的按钮。
    ///
    /// 返回 `Paused` 时调用方必须直接结束本次运行，不要再往后推进。
    pub fn prepare_stage(
        &mut self,
        run: &mut PipelineRun,
        stage_index: usize,
        skip_pause: bool,
        events: &dyn RunEvents,
    ) -> StagePlan {
        use crate::agent::state_machine::AgentEvent;

        let stage = run.pipeline[stage_index].clone();
        if stage.pause_before && !skip_pause {
            mark_pipeline_stage(&mut run.pipeline, stage_index, "paused");
            self.paused_run = Some(PausedPipelineRun {
                prompt: run.prompt.clone(),
                context: run.ctx_str.clone(),
                context_summary: run.context_summary.clone(),
                transcript: run.transcript.clone(),
                pipeline: run.pipeline.clone(),
                stage_index,
                ide_mode: run.ide_mode,
            });
            self.emit_pipeline(events, &run.pipeline);
            self.emit_action_log(
                events,
                "info",
                "stage_paused",
                Some(stage.role.to_string()),
                Some(&stage.name),
                &format!("Paused before {}", stage.name),
                "Pipeline paused before this stage by user configuration. Disable pause before this stage and rerun or continue with single-step controls.",
                Some(run.context_summary.clone()),
                Some(self.summarize_pending_diffs()),
            );
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::WaitingUser);
            self.emit_state(events);
            return StagePlan::Paused;
        }

        mark_pipeline_stage(&mut run.pipeline, stage_index, "active");
        self.emit_pipeline(events, &run.pipeline);
        self.emit_action_log(
            events,
            "info",
            "stage_start",
            Some(stage.role.to_string()),
            Some(&stage.name),
            &format!("{} stage started", stage.name),
            &format!(
                "Role: {}\nStage index: {}",
                stage.role.to_string(),
                stage_index + 1
            ),
            Some(run.context_summary.clone()),
            Some(self.summarize_pending_diffs()),
        );

        let step_index = self.ensure_stage_step(&stage);
        self.steps[step_index].status = "doing".to_string();
        self.steps[step_index]
            .logs
            .push(format!("{} stage started", stage.role.to_string()));
        self.emit_step(events, step_index);

        let _ = self
            .state_mgr
            .transition(&AgentEvent::StepStart(stage.name.clone()));
        self.emit_state(events);

        StagePlan::Ready {
            stage,
            step_id: self.steps[step_index].id.clone(),
            // 裁剪在 `execute_stage` 里做，这里保留完整线程：暂停/续跑要恢复的是
            // 全部历史，而不是某一次已经裁过的快照
            pending_diff_summary: self.summarize_pending_diffs(),
            // 克隆出 Arc 而不是借 `self`：调用方要在 await 期间放开锁，
            // 借用会把 orchestrator 的生命周期钉在整个阶段上
            tool_invoker: self.tool_invoker.clone(),
        }
    }

    /// 收敛一个阶段的结果，同步完成。
    ///
    /// **锁不变量**：整个函数必须在**一个**临界区里跑完。成功路径要一起落地
    /// 「step 状态 + transcript + 新 diff + 阶段标记」；这几样是同一份事实的不同
    /// 投影，拆开会让审查区里出现"diff 已到但阶段还没完成"之类的自相矛盾状态。
    ///
    /// 阶段执行期间没有持锁，所以 `step_id` 可能已经不存在了（`stop_agent` 清了
    /// 计划）。那种情况按"已取消"处理：不落地任何结果。
    #[allow(clippy::too_many_arguments)]
    pub fn record_stage_outcome(
        &mut self,
        run: &mut PipelineRun,
        stage_index: usize,
        stage: &PipelineStage,
        step_id: &str,
        outcome: Result<executor::StageOutcome, String>,
        cancel_flag: &Arc<AtomicBool>,
        events: &dyn RunEvents,
    ) -> Result<(), String> {
        use crate::agent::state_machine::AgentEvent;

        let Some(step_index) = self.steps.iter().position(|step| step.id == step_id) else {
            return Err(CANCELLED_ERROR.to_string());
        };

        match outcome {
            Ok(outcome) => {
                let response = outcome.text;
                self.steps[step_index].status = "done".to_string();
                self.steps[step_index].logs.push(format!(
                    "{} response: {}...",
                    stage.role.to_string(),
                    response.chars().take(200).collect::<String>()
                ));
                // 给这个 stage 的最后一条消息打上出处标签：下一个 stage 需要知道
                // 哪一句是哪个角色说的，光靠消息顺序看不出来
                let mut stage_messages = outcome.transcript;
                if let Some(last) = stage_messages.last_mut() {
                    last.content = format!(
                        "[{} / {}]\n{}",
                        stage.name,
                        stage.role.to_string(),
                        last.content
                    );
                }
                run.transcript.extend(stage_messages);

                let generated_diff_count = if run.ide_mode == IdeMode::Plan {
                    self.handle_plan_stage_response(
                        events,
                        stage,
                        &response,
                        &run.prompt,
                        run.context_summary.clone(),
                    )
                } else {
                    let parsed = executor::parse_diffs_with_diagnostics(&response);
                    let mut step_diffs = parsed.diffs;
                    attach_stage_provenance(&mut step_diffs, stage.role.to_string(), &stage.name);
                    // 生成时记录目标文件的内容指纹，apply 时才能识别期间发生的外部改动
                    crate::agent::diff_apply::stamp_base_hashes(&mut step_diffs);
                    let generated_diff_count = step_diffs.len();
                    self.diffs.extend(step_diffs);
                    if !parsed.diagnostics.is_empty() {
                        self.emit_action_log(
                            events,
                            "warn",
                            "agent_changes_validation",
                            Some(stage.role.to_string()),
                            Some(&stage.name),
                            "Agent changes validation reported issues",
                            &parsed.diagnostics.join("\n"),
                            Some(run.context_summary.clone()),
                            Some(self.summarize_pending_diffs()),
                        );
                    }
                    generated_diff_count
                };
                mark_pipeline_stage(&mut run.pipeline, stage_index, "completed");
                self.emit_action_log(
                    events,
                    "success",
                    "stage_complete",
                    Some(stage.role.to_string()),
                    Some(&stage.name),
                    &format!(
                        "{} stage completed with {} new {}{}",
                        stage.name,
                        generated_diff_count,
                        if run.ide_mode == IdeMode::Plan {
                            "artifact"
                        } else {
                            "diff"
                        },
                        if generated_diff_count == 1 { "" } else { "s" }
                    ),
                    &response,
                    Some(run.context_summary.clone()),
                    Some(self.summarize_pending_diffs()),
                );
            }
            Err(e) => {
                self.steps[step_index].status = "error".to_string();
                self.steps[step_index].logs.push(format!("Error: {}", e));
                mark_pipeline_stage(&mut run.pipeline, stage_index, "failed");
                self.emit_step(events, step_index);
                self.emit_pipeline(events, &run.pipeline);
                self.emit_action_log(
                    events,
                    "error",
                    "stage_error",
                    Some(stage.role.to_string()),
                    Some(&stage.name),
                    &format!("{} stage failed", stage.name),
                    &e,
                    Some(run.context_summary.clone()),
                    Some(self.summarize_pending_diffs()),
                );
                return Err(e);
            }
        }

        self.ensure_not_cancelled(cancel_flag, events)?;
        self.emit_step(events, step_index);
        self.emit_pipeline(events, &run.pipeline);

        let _ = self
            .state_mgr
            .transition(&AgentEvent::StepDone(stage.name.clone()));
        self.emit_state(events);

        Ok(())
    }

    /// 所有阶段跑完后的收尾，同步完成。
    ///
    /// **锁不变量**：整个函数必须在**一个**临界区里跑完。Auto 模式在这里
    /// 压回滚点、翻 diff 状态、再置终态；`apply_diffs_to_fs` 自身的不变量
    /// （见其文档）要求它和随后的状态翻转不被打断。
    pub fn finish_pipeline(
        &mut self,
        run: &PipelineRun,
        events: &dyn RunEvents,
    ) -> Result<(), String> {
        use crate::agent::state_machine::AgentEvent;

        // Auto applies diffs immediately; other modes wait for review.
        if run.ide_mode == IdeMode::Plan {
            if let Some(artifact) = self.sdd_artifacts.last() {
                events.emit_json(
                    "agent-sdd-ready",
                    serde_json::to_value(artifact).unwrap_or_default(),
                );

                self.emit_action_log(
                    events,
                    "info",
                    "sdd_ready",
                    None,
                    None,
                    &format!("SDD draft ready: {}", artifact.title),
                    "Plan mode completed without producing file diffs.",
                    Some(run.context_summary.clone()),
                    None,
                );
            }
            if let Some(artifact) = self.sdd_artifacts.last().cloned() {
                let _ = self.state_mgr.transition(&AgentEvent::SddReady(artifact));
            }
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::WaitingUser);
            self.emit_state(events);
            return Ok(());
        }

        if !self.diffs.is_empty() {
            events.emit_json(
                "agent-diff-ready",
                serde_json::to_value(&self.diffs).unwrap_or_default(),
            );
            self.emit_action_log(
                events,
                "info",
                "diff_ready",
                None,
                None,
                &format!(
                    "{} pending diff{} ready for review",
                    self.pending_diff_count(),
                    if self.pending_diff_count() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ),
                "Diff review is waiting for user action.",
                Some(run.context_summary.clone()),
                Some(self.summarize_pending_diffs()),
            );
        }
        let _ = self
            .state_mgr
            .transition(&AgentEvent::DiffReady(self.diffs.clone()));

        if self.mode == AgentMode::Auto {
            // Auto mode applies diffs immediately.
            let blocked = self.apply_diffs_to_fs()?;
            let (level, summary, details) = if blocked.is_empty() {
                (
                    "success",
                    "Auto mode applied pending diffs".to_string(),
                    "Agent auto mode completed filesystem apply.".to_string(),
                )
            } else {
                (
                    "warn",
                    format!(
                        "Auto mode applied edits but held {} new file{} for review",
                        blocked.len(),
                        if blocked.len() == 1 { "" } else { "s" }
                    ),
                    format!(
                        "File creation is not permitted for this run, so these stay pending:\n{}",
                        blocked.join("\n")
                    ),
                )
            };
            self.emit_action_log(
                events,
                level,
                "auto_apply",
                None,
                None,
                &summary,
                &details,
                Some(run.context_summary.clone()),
                Some(self.summarize_pending_diffs()),
            );
            // 还有被拦下的新建文件时不能算 Done，否则用户看不到需要审查的内容
            if blocked.is_empty() {
                self.state_mgr
                    .set(crate::agent::state_machine::AgentState::Done);
            } else {
                self.state_mgr
                    .set(crate::agent::state_machine::AgentState::WaitingUser);
            }
        } else {
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::WaitingUser);
        }
        self.emit_state(events);

        Ok(())
    }

    /// Apply pending diffs to the workspace filesystem.
    ///
    /// 返回被权限拦下、仍保持 pending 的文件路径。这不是失败：新建文件的
    /// diff 在未授权时留给人工审查，编辑已有文件照常应用。
    ///
    /// **锁不变量**：这个函数从头到尾必须在**一个**临界区里跑完。它先压回滚点
    /// （920）再翻转 diff 状态（922-928），两步之间不能让别的命令插进来 ——
    /// 插进来的 `undo_last_apply` 会弹掉这个刚压进去的 checkpoint，而此时 diff 还
    /// 写着 `pending`：磁盘已经改了，审查区说没改，撤销栈里也没有那一笔。
    ///
    /// 之后如果把 orchestrator 拆成多把锁（见 ROADMAP 的锁粒度条目），
    /// `diffs` 和 `undo_stack` 必须共用一把，或者这里显式按固定顺序取两把。
    pub fn apply_diffs_to_fs(&mut self) -> Result<Vec<String>, String> {
        let mut blocked: Vec<String> = Vec::new();
        let applicable: Vec<crate::agent::state_machine::FileDiff> = self
            .diffs
            .iter()
            .filter(|diff| diff.status == "pending")
            .filter(|diff| {
                if self.allow_file_create || !crate::agent::diff_apply::is_new_file_diff(diff) {
                    return true;
                }
                blocked.push(diff.file.clone());
                false
            })
            .cloned()
            .collect();

        let (result, snapshots) =
            crate::agent::diff_apply::apply_pending_diffs_with_snapshots(&applicable);
        self.push_undo_checkpoint("Auto-apply", snapshots);

        for diff in &mut self.diffs {
            if result.applied.iter().any(|item| item.id == diff.id) {
                diff.status = "applied".to_string();
            } else if result.failed.iter().any(|item| item.diff_id == diff.id) {
                diff.status = "failed".to_string();
            }
        }

        if !result.failed.is_empty() {
            return Err(result
                .failed
                .iter()
                .map(|item| format!("{}: {}", item.file, item.message))
                .collect::<Vec<_>>()
                .join("; "));
        }

        Ok(blocked)
    }

    /// 拒绝一个 diff 里所有还没决定的 hunk。
    ///
    /// 和 `apply_diff` 对称：接受 `partial` / `failed`，这样"先应用一个 hunk、
    /// 再拒绝剩下的"是可行的。已 applied 的 hunk 保持不变；整体状态由
    /// `status_from_hunks` 推导，而不是硬写成 "rejected" —— 后者会让文件状态
    /// 和各 hunk 状态互相矛盾。
    pub fn reject_diff(
        &mut self,
        diff_id: &str,
    ) -> Result<crate::agent::state_machine::FileDiff, String> {
        let Some(diff) = self.diffs.iter_mut().find(|item| item.id == diff_id) else {
            return Err(format!("Diff not found: {}", diff_id));
        };

        if diff.status != "pending" && diff.status != "partial" && diff.status != "failed" {
            return Err(format!(
                "Diff {} cannot be rejected while status is {}",
                diff_id, diff.status
            ));
        }

        let mut rejected_any = false;
        for hunk in &mut diff.hunks {
            if matches!(hunk.status.as_deref(), Some("applied") | Some("rejected")) {
                continue;
            }
            hunk.status = Some("rejected".to_string());
            rejected_any = true;
        }

        if !rejected_any {
            return Err(format!("Diff {} has no hunks left to reject", diff_id));
        }

        diff.status = status_from_hunks(&diff.hunks);
        let updated = diff.clone();
        self.refresh_review_state();

        Ok(updated)
    }

    /// 拒绝单个 hunk。
    pub fn reject_diff_hunk(
        &mut self,
        diff_id: &str,
        hunk_index: usize,
    ) -> Result<crate::agent::state_machine::FileDiff, String> {
        let Some(diff) = self.diffs.iter_mut().find(|item| item.id == diff_id) else {
            return Err(format!("Diff not found: {}", diff_id));
        };

        if diff.status != "pending" && diff.status != "partial" && diff.status != "failed" {
            return Err(format!(
                "Diff {} cannot reject hunks while status is {}",
                diff_id, diff.status
            ));
        }

        let Some(hunk) = diff.hunks.get_mut(hunk_index) else {
            return Err(format!("Hunk {} not found in diff {}", hunk_index, diff_id));
        };

        if matches!(hunk.status.as_deref(), Some("applied") | Some("rejected")) {
            return Err(format!(
                "Hunk {} in diff {} is already {}",
                hunk_index,
                diff_id,
                hunk.status.clone().unwrap_or_default()
            ));
        }

        hunk.status = Some("rejected".to_string());
        diff.status = status_from_hunks(&diff.hunks);
        let updated = diff.clone();
        self.refresh_review_state();

        Ok(updated)
    }

    /// 应用一个 diff 里所有还没决定的 hunk。
    ///
    /// 接受 `partial` / `failed`，而不是只接受 `pending`：逐 hunk 审查过之后
    /// 整文件 Apply 仍然应该能把剩下的一次性落地。已 applied / rejected 的
    /// hunk 会被跳过，不会二次写入。
    pub fn apply_diff(
        &mut self,
        diff_id: &str,
    ) -> Result<crate::agent::state_machine::ApplyDiffsResult, String> {
        let Some(diff) = self.diffs.iter().find(|item| item.id == diff_id).cloned() else {
            return Err(format!("Diff not found: {}", diff_id));
        };

        if diff.status != "pending" && diff.status != "partial" && diff.status != "failed" {
            return Err(format!(
                "Diff {} cannot be applied while status is {}",
                diff_id, diff.status
            ));
        }

        let undecided: Vec<usize> = diff
            .hunks
            .iter()
            .enumerate()
            .filter(|(_, hunk)| {
                !matches!(hunk.status.as_deref(), Some("applied") | Some("rejected"))
            })
            .map(|(index, _)| index)
            .collect();

        if undecided.is_empty() {
            return Err(format!("Diff {} has no hunks left to apply", diff_id));
        }

        let synthetic = crate::agent::state_machine::FileDiff {
            hunks: undecided
                .iter()
                .map(|index| diff.hunks[*index].clone())
                .collect(),
            // 同 apply_diff_hunk：apply_pending_diffs 只处理 pending
            status: "pending".to_string(),
            ..diff.clone()
        };
        let (result, snapshots) =
            crate::agent::diff_apply::apply_pending_diffs_with_snapshots(&[synthetic]);
        self.push_undo_checkpoint(&format!("Apply file {}", diff.file), snapshots);

        if let Some(item) = self.diffs.iter_mut().find(|item| item.id == diff_id) {
            let applied = result.applied.iter().any(|entry| entry.id == item.id);
            let failed = result.failed.iter().any(|entry| entry.diff_id == item.id);
            if applied || failed {
                let status = if applied { "applied" } else { "failed" };
                for index in &undecided {
                    if let Some(hunk) = item.hunks.get_mut(*index) {
                        hunk.status = Some(status.to_string());
                    }
                }
                item.status = status_from_hunks(&item.hunks);
            }
        }
        crate::agent::diff_apply::restamp_applied_files(&mut self.diffs, &result.applied);
        self.refresh_review_state();

        Ok(result)
    }

    /// 逐 hunk 应用一个待审查 diff。
    ///
    /// 这里是纯状态操作 + 文件写入，不碰 IPC：Tauri 命令层只负责加锁、
    /// 发事件和写 action log。这样这条链路（含 baseHash 重新盖章）可以直接
    /// 用单测覆盖，而不必靠人点界面。
    pub fn apply_diff_hunk(
        &mut self,
        diff_id: &str,
        hunk_index: usize,
    ) -> Result<crate::agent::state_machine::ApplyDiffsResult, String> {
        let Some(diff) = self.diffs.iter().find(|item| item.id == diff_id).cloned() else {
            return Err(format!("Diff not found: {}", diff_id));
        };

        if diff.status != "pending" && diff.status != "partial" && diff.status != "failed" {
            return Err(format!(
                "Diff {} cannot apply hunks while status is {}",
                diff_id, diff.status
            ));
        }

        let Some(hunk) = diff.hunks.get(hunk_index).cloned() else {
            return Err(format!("Hunk {} not found in diff {}", hunk_index, diff_id));
        };

        if hunk.status.as_deref() == Some("applied") || hunk.status.as_deref() == Some("rejected") {
            return Err(format!(
                "Hunk {} in diff {} is already {}",
                hunk_index,
                diff_id,
                hunk.status.unwrap_or_default()
            ));
        }

        let single_hunk_diff = crate::agent::state_machine::FileDiff {
            hunks: vec![hunk],
            // 必须显式置为 pending：`apply_pending_diffs` 会跳过非 pending 的 diff，
            // 而这里继承来的状态在应用过第一个 hunk 之后是 "partial"。
            // 不重置的话第二个 hunk 会被静默跳过 —— applied 和 failed 都为空，
            // 命令返回 Ok，用户既看不到变更也看不到错误。
            status: "pending".to_string(),
            ..diff.clone()
        };
        let (result, snapshots) =
            crate::agent::diff_apply::apply_pending_diffs_with_snapshots(&[single_hunk_diff]);
        self.push_undo_checkpoint(
            &format!("Apply hunk {} in {}", hunk_index + 1, diff.file),
            snapshots,
        );

        if let Some(item) = self.diffs.iter_mut().find(|item| item.id == diff_id) {
            if result.applied.iter().any(|applied| applied.id == item.id) {
                if let Some(hunk) = item.hunks.get_mut(hunk_index) {
                    hunk.status = Some("applied".to_string());
                }
                item.status = status_from_hunks(&item.hunks);
            } else if result
                .failed
                .iter()
                .any(|failure| failure.diff_id == item.id)
            {
                if let Some(hunk) = item.hunks.get_mut(hunk_index) {
                    hunk.status = Some("failed".to_string());
                }
                item.status = "failed".to_string();
            }
        }
        // 逐 hunk 应用会推进文件内容，后续 hunk 必须以新内容为基准，否则会被误判 stale
        crate::agent::diff_apply::restamp_applied_files(&mut self.diffs, &result.applied);
        self.refresh_review_state();

        Ok(result)
    }

    /// 还有待处理的 diff 时留在 WaitingUser，否则收尾为 Done
    pub fn refresh_review_state(&mut self) {
        let has_open_work = self.diffs.iter().any(|diff| {
            diff.status == "pending" || diff.status == "partial" || diff.status == "failed"
        });
        if has_open_work {
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::WaitingUser);
        } else {
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::Done);
        }
    }
}

/// 由各 hunk 的状态推导整个文件 diff 的状态
pub fn status_from_hunks(hunks: &[crate::agent::state_machine::DiffHunk]) -> String {
    let all_match = |expected: &str| {
        !hunks.is_empty()
            && hunks
                .iter()
                .all(|hunk| hunk.status.as_deref() == Some(expected))
    };
    let any_match = |expected: &str| {
        hunks
            .iter()
            .any(|hunk| hunk.status.as_deref() == Some(expected))
    };

    if all_match("applied") {
        "applied".to_string()
    } else if all_match("rejected") {
        "rejected".to_string()
    } else if any_match("failed") {
        "failed".to_string()
    } else if any_match("applied") || any_match("rejected") {
        "partial".to_string()
    } else {
        "pending".to_string()
    }
}

impl AgentOrchestrator {
    fn handle_plan_stage_response(
        &mut self,
        events: &dyn RunEvents,
        stage: &PipelineStage,
        response: &str,
        prompt: &str,
        context_summary: String,
    ) -> usize {
        if stage.role == AgentRole::Designer {
            let artifact = executor::parse_sdd_artifact(
                response,
                prompt,
                self.last_run_id
                    .clone()
                    .or_else(|| self.current_run_id.clone()),
            );
            self.sdd_artifacts.push(artifact.clone());
            events.emit_json(
                "agent-sdd-ready",
                serde_json::to_value(&artifact).unwrap_or_default(),
            );
            self.emit_action_log(
                events,
                "success",
                "sdd_draft",
                Some(stage.role.to_string()),
                Some(&stage.name),
                &format!("SDD draft produced: {}", artifact.title),
                &artifact.markdown,
                Some(context_summary),
                None,
            );
            1
        } else if stage.role == AgentRole::Reviewer {
            let findings = executor::extract_review_findings(response);
            if !findings.is_empty() {
                if let Some(artifact) = self.sdd_artifacts.last_mut() {
                    artifact.review_findings.extend(findings);
                    artifact.status = "reviewed".to_string();
                    events.emit_json(
                        "agent-sdd-ready",
                        serde_json::to_value(artifact).unwrap_or_default(),
                    );
                }
            }
            0
        } else {
            0
        }
    }

    /// `agent-state-changed` 的规范 payload。所有发这个事件的地方都必须走这里。
    ///
    /// 以前 12 个发送点各自手写 `json!`：有的只带 `state`，有的带五个字段，而前端
    /// 无条件读 `ideMode` —— 同一个事件的形状取决于谁发的。
    ///
    /// 更要紧的是 `pendingUndo`。撤销可用性必须随每次状态变化一起送出去，否则界面
    /// 只能反过来查询，而查询要抢 orchestrator 锁 —— 运行期间那把锁被整条流水线占着。
    /// 放进 payload 就没有这个问题：它由刚刚改完撤销栈的同一段代码在同一个临界区里
    /// 计算，物理上不可能和真实栈漂移，也不需要第二份事实来源。
    /// 而如果把这个字段散在 12 处手写，一定会漏，漏掉的那处会让撤销按钮静默停在旧值。
    ///
    /// `usage` 同理搭这趟车。它本来只在运行**结束**时写进一条 action log，也就是
    /// "这次花了多少钱"要等跑完、还得去翻日志。放进这个 payload 之后状态栏能一直
    /// 显示。刻意**不**为它新开事件、也不让 `RunUsageMeter` 去持有 `RunEvents`：
    /// 代价是刷新粒度只到状态变化（planner、每个 stage 的起止），一个 stage 内部
    /// 的多轮工具调用要等这个 stage 结束才反映出来。这个精度对状态栏够用，而把
    /// 发事件的职责塞进 llm_client 会把 Tauri 那一侧的关注点漏进最底层。
    pub fn state_payload(&self) -> serde_json::Value {
        serde_json::json!({
            "state": self.state_mgr.state.to_string(),
            "mode": self.mode.to_string(),
            "ideMode": self.ide_mode.to_string(),
            "currentRunId": self.current_run_id,
            "lastRunId": self.last_run_id,
            "pendingUndo": self.pending_undo().map(|(label, files)| serde_json::json!({
                "label": label,
                "files": files,
            })),
            // 只送数字，不送措辞：`calls`/`reportedCalls` 让前端自己判断
            // "完全没回报"和"部分回报"，因为 24px 的状态栏和 action log
            // 需要的说法不一样。`spendMicros` 为 null 是"没配价格算不出来"，
            // 不是"没花钱" —— 前端必须保住这个区别。
            "usage": self.run_usage.as_ref().map(|meter| {
                let snapshot = meter.snapshot();
                serde_json::json!({
                    "totalTokens": snapshot.total_tokens,
                    "maxTotalTokens": snapshot.max_total_tokens,
                    "spendMicros": snapshot.spend_micros,
                    "maxSpendMicros": snapshot.max_spend_micros,
                    "calls": snapshot.calls,
                    "reportedCalls": snapshot.reported_calls,
                })
            }),
        })
    }

    /// Emit the current state to the frontend.
    fn emit_state(&self, events: &dyn RunEvents) {
        events.emit_json("agent-state-changed", self.state_payload());
    }

    fn emit_pipeline(&self, events: &dyn RunEvents, pipeline: &[PipelineStage]) {
        events.emit_json(
            "agent-pipeline-update",
            serde_json::to_value(pipeline).unwrap_or_default(),
        );
    }

    fn emit_step(&self, events: &dyn RunEvents, step_index: usize) {
        if let Some(step) = self.steps.get(step_index) {
            events.emit_json(
                "agent-step-update",
                serde_json::to_value(step).unwrap_or_default(),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_action_log(
        &self,
        events: &dyn RunEvents,

        level: &str,
        phase: &str,
        role: Option<&str>,
        stage: Option<&str>,
        summary: &str,
        details: &str,
        context_summary: Option<String>,
        diff_summary: Option<String>,
    ) {
        let entry = ActionLogEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: level.to_string(),
            phase: phase.to_string(),
            role: role.map(str::to_string),
            stage: stage.map(str::to_string),
            summary: summary.to_string(),
            details: details.to_string(),
            context_summary,
            diff_summary,
        };
        events.emit_json(
            "agent-action-log",
            serde_json::to_value(entry).unwrap_or_default(),
        );
    }

    pub fn emit_review_action_log(
        &self,
        events: &dyn RunEvents,
        level: &str,
        phase: &str,
        summary: &str,
        details: &str,
    ) {
        self.emit_action_log(
            events,
            level,
            phase,
            None,
            Some("Diff Review"),
            summary,
            details,
            None,
            Some(self.summarize_pending_diffs()),
        );
    }

    fn pending_diff_count(&self) -> usize {
        self.diffs
            .iter()
            .filter(|diff| is_reviewable_diff_status(&diff.status))
            .count()
    }

    fn summarize_pending_diffs(&self) -> String {
        let pending: Vec<_> = self
            .diffs
            .iter()
            .filter(|diff| is_reviewable_diff_status(&diff.status))
            .collect();

        if pending.is_empty() {
            return "No reviewable diffs.".to_string();
        }

        let mut lines = Vec::new();
        lines.push(format!("Reviewable diffs: {}", pending.len()));
        for diff in pending {
            lines.push(format!(
                "- {} [{}]: {} hunk{}",
                diff.file,
                diff.status,
                diff.hunks.len(),
                if diff.hunks.len() == 1 { "" } else { "s" }
            ));
            for (index, hunk) in diff.hunks.iter().enumerate() {
                lines.push(format!(
                    "  Hunk {} [{}]: -{} lines, +{} lines",
                    index + 1,
                    hunk.status.as_deref().unwrap_or("pending"),
                    hunk.old_lines,
                    hunk.new_lines
                ));
                if !hunk.original.trim().is_empty() {
                    lines.push(format!(
                        "  Original excerpt: {}",
                        summarize_text(&hunk.original, 180)
                    ));
                }
                if !hunk.updated.trim().is_empty() {
                    lines.push(format!(
                        "  Updated excerpt: {}",
                        summarize_text(&hunk.updated, 180)
                    ));
                }
            }
        }
        lines.join("\n")
    }

    fn ensure_stage_step(&mut self, stage: &PipelineStage) -> usize {
        if let Some(index) = self.steps.iter().position(|step| step.title == stage.name) {
            return index;
        }

        self.steps.push(TaskStep {
            id: uuid::Uuid::new_v4().to_string(),
            title: stage.name.clone(),
            step_type: stage.role.to_string().to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
            scope: None,
            execution_mode: None,
        });
        self.steps.len() - 1
    }

    fn ensure_not_cancelled(
        &mut self,
        cancel_flag: &Arc<AtomicBool>,
        events: &dyn RunEvents,
    ) -> Result<(), String> {
        if cancel_flag.load(Ordering::SeqCst) {
            self.state_mgr
                .set(crate::agent::state_machine::AgentState::Idle);
            self.emit_state(events);

            return Err(CANCELLED_ERROR.to_string());
        }
        Ok(())
    }

    /// 批量应用所有还可审查的 diff（界面上的 Apply all）。
    ///
    /// 逐文件复用 `apply_diff`，而不是把 `self.diffs` 整个交给
    /// `apply_pending_diffs`：后者只处理 `pending`，所以任何逐 hunk 审查过的
    /// 文件都会被整批跳过 —— 不报错、不落地、汇总里也看不出来。复用还顺带
    /// 保证被拒绝的 hunk 不会被批量应用重新写回去，并让 hunk 级状态和文件
    /// 状态保持一致（旧实现只改文件状态，hunk 状态留空）。
    pub fn apply_all_diffs(&mut self) -> crate::agent::state_machine::ApplyDiffsResult {
        let mut applied = Vec::new();
        let mut failed = Vec::new();

        for (id, file) in self.reviewable_diff_targets() {
            match self.apply_diff(&id) {
                Ok(result) => {
                    applied.extend(result.applied);
                    failed.extend(result.failed);
                }
                // 已经按 undecided 过滤过，走到这里说明状态机自身不一致，
                // 报出来而不是静默跳过
                Err(message) => failed.push(crate::agent::state_machine::ApplyDiffError {
                    diff_id: id,
                    file,
                    message,
                }),
            }
        }

        self.refresh_review_state();
        crate::agent::state_machine::ApplyDiffsResult { applied, failed }
    }

    /// 批量拒绝所有还可审查的 diff（界面上的 Reject all）。
    ///
    /// 返回的是本次真正改动过的 diff。旧实现返回所有 status == "rejected" 的
    /// diff，会把上一轮拒绝的也算进来，action log 的条数因此偏大。
    pub fn reject_all_diffs(&mut self) -> Vec<crate::agent::state_machine::FileDiff> {
        let mut rejected = Vec::new();
        for (id, _) in self.reviewable_diff_targets() {
            if let Ok(diff) = self.reject_diff(&id) {
                rejected.push(diff);
            }
        }
        self.refresh_review_state();
        rejected
    }

    /// 进入下一轮修复前的全部状态变更，同步完成。
    ///
    /// **锁不变量**：整个函数必须在**一个**临界区里跑完。它要么写完"停止"那条
    /// action log 并交出结果，要么把这一轮的 step 登记成 doing 并发出去 ——
    /// 中间被别的命令插入，界面上会出现一个没有归属的 doing 步骤。
    ///
    /// 返回 `Done` 时调用方必须直接结束循环。
    pub fn prepare_repair_iteration(
        &mut self,
        run: &mut RepairRun,
        cancel: &Arc<AtomicBool>,
        events: &dyn RunEvents,
    ) -> Result<RepairPlan, String> {
        use crate::services::verification::{
            build_repair_prompt, collect_command_problems, RepairDecision,
        };

        let iteration = match run
            .policy
            .next(run.completed, run.checks_failed, run.apply_failed)
        {
            RepairDecision::Repair { iteration } => iteration,
            RepairDecision::Stop(stop) => {
                // 一轮都没修过就不写这条记录：一次检查全过的运行里，
                // "repair not enabled" 只是噪音
                if run.completed > 0 {
                    self.emit_action_log(
                        events,
                        if run.checks_failed { "warn" } else { "success" },
                        "repair_loop",
                        None,
                        Some("Repair"),
                        &format!(
                            "Repair stopped after {} iteration(s): {}",
                            run.completed,
                            stop.reason()
                        ),
                        &format!("Checks still failing: {}", run.checks_failed),
                        None,
                        None,
                    );
                }
                return Ok(RepairPlan::Done(RepairLoopOutcome {
                    iterations: run.completed,
                    stop,
                    checks_failed: run.checks_failed,
                    results: std::mem::take(&mut run.results),
                }));
            }
        };

        self.ensure_not_cancelled(cancel, events)?;
        let problems = collect_command_problems(&run.results);
        let prompt = build_repair_prompt(
            &run.original_prompt,
            iteration,
            &run.results,
            &problems,
        );
        let step = TaskStep {
            id: format!("repair-{}-{}", iteration, uuid::Uuid::new_v4()),
            title: format!("Repair failed checks ({})", iteration),
            step_type: "edit".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
            scope: Some("workspace".to_string()),
            execution_mode: Some("fix".to_string()),
        };
        self.begin_step(&step, "Repair iteration started");
        self.emit_step(events, self.steps.len().saturating_sub(1));

        Ok(RepairPlan::Iterate {
            iteration,
            step,
            prompt,
            // 克隆出 Arc 而不是借 `self`：调用方要在 await 期间放开锁
            tool_invoker: self.tool_invoker.clone(),
        })
    }

    /// 记下模型的回答并把这一轮的改动落盘，同步完成。返回 (成功, 失败) 文件数。
    ///
    /// **锁不变量**：整个函数必须在**一个**临界区里跑完。`record_step_success`
    /// 和 `apply_all_diffs` 之间被插入的话，审查区里会出现"这一轮已完成、但改动
    /// 还没落盘"的状态，而紧接着的重跑检查看的就是没落盘的代码。
    ///
    /// 落盘走 `apply_all_diffs` 而不是绕过审查区直接写文件：逐文件、带 base hash
    /// 校验和回滚点，所以循环结束后每一轮的改动仍然可以 Undo Apply。
    pub fn record_repair_apply(
        &mut self,
        run: &mut RepairRun,
        step: &TaskStep,
        response: &str,
    ) -> (usize, usize) {
        self.record_step_success(step, response, None, None);
        let applied = self.apply_all_diffs();
        run.apply_failed = !applied.failed.is_empty();
        (applied.applied.len(), applied.failed.len())
    }

    /// 一轮修复的收尾日志，同步完成。
    pub fn record_repair_iteration(
        &mut self,
        run: &RepairRun,
        iteration: u8,
        applied_count: usize,
        failed_count: usize,
        events: &dyn RunEvents,
    ) {
        self.emit_action_log(
            events,
            if run.checks_failed { "warn" } else { "success" },
            "repair_iteration",
            None,
            Some("Repair"),
            &format!(
                "Repair iteration {}: checks {}",
                iteration,
                if run.checks_failed {
                    "still failing"
                } else {
                    "pass"
                }
            ),
            &format!(
                "Applied {} file(s), {} failed to apply",
                applied_count, failed_count
            ),
            None,
            None,
        );
    }

    /// 还有未决 hunk、因而值得批量处理的 diff：(id, file)
    fn reviewable_diff_targets(&self) -> Vec<(String, String)> {
        self.diffs
            .iter()
            .filter(|diff| is_reviewable_diff_status(&diff.status))
            .filter(|diff| {
                diff.hunks.iter().any(|hunk| {
                    !matches!(hunk.status.as_deref(), Some("applied") | Some("rejected"))
                })
            })
            .map(|diff| (diff.id.clone(), diff.file.clone()))
            .collect()
    }

    /// 单步开始：把步骤登记为 doing，返回登记后的副本供命令层发事件
    pub fn begin_step(&mut self, step: &TaskStep, log: &str) -> TaskStep {
        self.record_step_status(step, "doing", log)
    }

    /// 更新（必要时插入）某个步骤的状态，返回登记后的副本
    pub fn record_step_status(&mut self, step: &TaskStep, status: &str, log: &str) -> TaskStep {
        let mut updated = step.clone();
        updated.status = status.to_string();
        updated.logs.push(log.to_string());
        self.upsert_step(updated.clone());
        updated
    }

    /// 单步成功：登记响应里的 diff 并收敛审查状态。
    ///
    /// 这里刻意用 `refresh_review_state` 而不是硬置 `WaitingUser`：一个只返回
    /// 文字、没有产出 diff 的步骤以前也会把界面留在"需要处理"，而审查区里
    /// 什么都没有，用户只能靠重跑脱身。
    pub fn record_step_success(
        &mut self,
        step: &TaskStep,
        response: &str,
        regenerated_from_diff_id: Option<&str>,
        regenerated_from_hunk_index: Option<usize>,
    ) -> StepRunOutcome {
        let mut step = step.clone();
        step.status = "done".to_string();
        step.logs.push(format!(
            "Single step response: {}...",
            response.chars().take(200).collect::<String>()
        ));
        self.upsert_step(step.clone());

        let parsed = executor::parse_diffs_with_diagnostics(response);
        let mut diffs = parsed.diffs;
        crate::services::agent_runtime::attach_step_provenance(
            &mut diffs,
            &step,
            regenerated_from_diff_id,
            regenerated_from_hunk_index,
        );
        // 生成时记录目标文件的内容指纹，apply 时才能识别期间发生的外部改动
        crate::agent::diff_apply::stamp_base_hashes(&mut diffs);
        let new_diffs = diffs.len();
        self.diffs.extend(diffs);
        self.refresh_review_state();

        StepRunOutcome {
            step,
            new_diffs,
            diagnostics: parsed.diagnostics,
        }
    }

    fn upsert_step(&mut self, step: TaskStep) {
        if let Some(existing) = self.steps.iter_mut().find(|item| item.id == step.id) {
            *existing = step;
        } else {
            self.steps.push(step);
        }
    }
}

/// 单步执行的结果摘要，供命令层发事件和写 action log
pub struct StepRunOutcome {
    pub step: TaskStep,
    /// 本次步骤新产出的 diff 数量（不含此前遗留的）
    pub new_diffs: usize,
    pub diagnostics: Vec<String>,
}

fn is_reviewable_diff_status(status: &str) -> bool {
    matches!(status, "pending" | "partial" | "failed")
}

fn summarize_text(text: &str, max_chars: usize) -> String {
    let normalized = text
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    let mut summary: String = normalized.chars().take(max_chars).collect();
    if normalized.chars().count() > max_chars {
        summary.push_str("...");
    }
    summary
}

fn format_context_budget_summary(
    budget: Option<&ContextBudget>,
    raw_chars: usize,
    final_chars: usize,
) -> String {
    let Some(budget) = budget else {
        return format!("Context budget: unset\nContext chars: {}", final_chars);
    };
    let estimated_input = estimated_input_tokens_from_budget(budget)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unset".to_string());
    format!(
        "Context budget: estimated input tokens={}, max context tokens={}, reserved output tokens={}\nContext chars: raw={}, final={}, trimmed={}",
        estimated_input,
        budget.max_context_tokens.map(|value| value.to_string()).unwrap_or_else(|| "unset".to_string()),
        budget.reserved_output_tokens.map(|value| value.to_string()).unwrap_or_else(|| "unset".to_string()),
        raw_chars,
        final_chars,
        final_chars < raw_chars
    )
}

fn format_context_sources(sources: &ContextSourceOptions) -> String {
    format!(
        "Context sources: projectTree={}, gitDiff={}, projectMemory={}",
        sources.include_project_tree, sources.include_git_diff, sources.include_project_memory
    )
}

fn attach_stage_provenance(
    diffs: &mut [crate::agent::state_machine::FileDiff],
    role: &str,
    stage: &str,
) {
    for diff in diffs {
        let provenance = diff.provenance.get_or_insert_with(|| DiffProvenance {
            protocol: "unknown".to_string(),
            operation: "unknown".to_string(),
            rationale: None,
            schema_version: None,
            change_index: None,
            source_role: None,
            source_stage: None,
            regenerated_from_diff_id: None,
            regenerated_from_hunk_index: None,
        });
        provenance.source_role = Some(role.to_string());
        provenance.source_stage = Some(stage.to_string());
        for (hunk_index, hunk) in diff.hunks.iter_mut().enumerate() {
            let hunk_provenance = hunk.provenance.get_or_insert_with(|| DiffHunkProvenance {
                change_index: provenance.change_index,
                hunk_index: Some(hunk_index),
                source_role: None,
                source_stage: None,
                prompt_context: None,
                rationale: provenance.rationale.clone(),
            });
            hunk_provenance.source_role = Some(role.to_string());
            hunk_provenance.source_stage = Some(stage.to_string());
            hunk_provenance.change_index = hunk_provenance.change_index.or(provenance.change_index);
            hunk_provenance.hunk_index = hunk_provenance.hunk_index.or(Some(hunk_index));
            if hunk_provenance.rationale.is_none() {
                hunk_provenance.rationale = provenance.rationale.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::state_machine::{DiffHunk, FileDiff};
    use crate::agent::workspace_tools::AgentFileWrite;
    use crate::services::workspace;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    /// 工具直接写盘会绕过审查区：文件变了而 Diff 视图空着，用户看不到 Agent
    /// 改了什么，也没有撤销入口。所以每次写入都要合成一张已应用的 diff 卡片，
    /// 并压一个回滚点。
    ///
    /// 同一文件写多次要合并成一条：original 取第一次写之前的内容，updated 取
    /// 最后一次写入的。撤销要回到"这次运行之前"，不是回到中间某一步。
    /// 审查区在界面上的说明全靠 action log，所以这里断言"发出了什么事件"，
    /// 而不只是"函数没 panic"：一次逻辑正确但没发事件的运行，在界面上等于没发生。
    ///
    /// 这条以前断言不了 —— `emit_review_action_log` 要 `AppHandle`，而 lib 测试里
    /// 拿不到（把 Tauri runtime 拉进测试二进制会让整个套件在加载阶段起不来）。
    /// 现在发事件是一个可替换的依赖，测试塞 `RecordingEvents` 就行。
    #[test]
    fn review_action_log_reports_the_pending_diffs_it_is_about() {
        let events = crate::agent::events::RecordingEvents::new();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs.push(FileDiff {
            id: "diff-1".to_string(),
            file: "src/app.ts".to_string(),
            base_hash: None,
            provenance: None,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                content: String::new(),
                original: "before\n".to_string(),
                updated: "after\n".to_string(),
                provenance: None,
                status: None,
            }],
            status: "pending".to_string(),
        });

        orchestrator.emit_review_action_log(
            &events,
            "info",
            "diff_ready",
            "1 file changed",
            "details",
        );

        let payloads = events.payloads_for("agent-action-log");
        assert_eq!(payloads.len(), 1);
        let entry = &payloads[0];
        assert_eq!(entry["level"], "info");
        assert_eq!(entry["phase"], "diff_ready");
        // 审查区的日志都挂在这个 stage 下，前端靠它分组
        assert_eq!(entry["stage"], "Diff Review");
        let diff_summary = entry["diffSummary"].as_str().unwrap_or_default();
        assert!(
            diff_summary.contains("src/app.ts"),
            "日志要指明是哪次改动，否则记录里看不出它在说什么: {}",
            diff_summary
        );

        // 没有可审查的 diff 时也要给一句明确的话，而不是空字符串
        let empty_events = crate::agent::events::RecordingEvents::new();
        AgentOrchestrator::new().emit_review_action_log(
            &empty_events,
            "info",
            "diff_ready",
            "nothing to review",
            "details",
        );
        let entry = &empty_events.payloads_for("agent-action-log")[0];
        assert_eq!(entry["diffSummary"], "No reviewable diffs.");
    }

    #[test]
    fn tool_writes_become_applied_diffs_with_an_undo_point() {
        let mut orchestrator = AgentOrchestrator::new();

        let recorded = orchestrator.record_tool_writes(vec![
            AgentFileWrite {
                file: "src/app.ts".to_string(),
                path: PathBuf::from("src/app.ts"),
                previous: Some("before run\n".to_string()),
                updated: "first write\n".to_string(),
            },
            AgentFileWrite {
                file: "src/app.ts".to_string(),
                path: PathBuf::from("src/app.ts"),
                previous: Some("first write\n".to_string()),
                updated: "second write\n".to_string(),
            },
            AgentFileWrite {
                file: "src/new.ts".to_string(),
                path: PathBuf::from("src/new.ts"),
                previous: None,
                updated: "created\n".to_string(),
            },
        ]);

        assert_eq!(recorded.len(), 2, "same file must merge into one entry");
        let edited = &recorded[0];
        assert_eq!(edited.file, "src/app.ts");
        assert_eq!(edited.status, "applied");
        assert_eq!(edited.hunks[0].status.as_deref(), Some("applied"));
        assert_eq!(edited.hunks[0].original, "before run\n");
        assert_eq!(edited.hunks[0].updated, "second write\n");
        assert_eq!(
            edited.provenance.as_ref().map(|p| p.operation.as_str()),
            Some("edit")
        );
        assert_eq!(
            recorded[1]
                .provenance
                .as_ref()
                .map(|p| p.operation.as_str()),
            Some("create")
        );

        // 回滚点必须存在，否则 Undo Apply 对工具写入无效
        let (label, files) = orchestrator.pending_undo().expect("undo checkpoint");
        assert!(label.contains("tool"), "{}", label);
        assert_eq!(files.len(), 2);

        // 空输入不该压出一个什么都没改的回滚点
        let mut fresh = AgentOrchestrator::new();
        assert!(fresh.record_tool_writes(Vec::new()).is_empty());
        assert!(fresh.pending_undo().is_none());
    }

    struct TestEnv {
        root: PathBuf,
        config_dir: PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let base = std::env::temp_dir()
                .join(format!("agent-ide-orchestrator-test-{}", Uuid::new_v4()));
            let root = base.join("workspace");
            let config_dir = base.join("config");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&config_dir).unwrap();
            let root = root.canonicalize().unwrap();
            std::env::set_var("AGENT_IDE_CONFIG_DIR", &config_dir);
            workspace::save_workspace_path(root.to_string_lossy().as_ref()).unwrap();
            Self { root, config_dir }
        }

        fn write_file(&self, relative: &str, content: &str) {
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
            let _ = std::fs::remove_dir_all(
                self.root
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.root.clone()),
            );
            let _ = std::fs::remove_dir_all(&self.config_dir);
        }
    }

    fn make_diff(file: &str, original: &str, updated: &str) -> FileDiff {
        FileDiff {
            id: Uuid::new_v4().to_string(),
            file: file.to_string(),
            base_hash: None,
            provenance: None,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                content: String::new(),
                original: original.to_string(),
                updated: updated.to_string(),
                provenance: None,
                status: None,
            }],
            status: "pending".to_string(),
        }
    }

    #[test]
    fn auto_apply_marks_partial_failure_and_returns_error() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("ok.ts", "const value = 1;\n");
        env.write_file("fail.ts", "const other = 1;\n");

        let ok = make_diff("ok.ts", "const value = 1;", "const value = 2;");
        let fail = make_diff("fail.ts", "const missing = 1;", "const missing = 2;");
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![ok.clone(), fail.clone()];

        let err = orchestrator.apply_diffs_to_fs().unwrap_err();

        assert!(err.contains("Could not find original content"));
        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(orchestrator.diffs[1].status, "failed");
        assert_eq!(
            std::fs::read_to_string(env.root.join("ok.ts")).unwrap(),
            "const value = 2;\n"
        );
        assert_eq!(
            std::fs::read_to_string(env.root.join("fail.ts")).unwrap(),
            "const other = 1;\n"
        );
    }

    #[test]
    fn auto_apply_holds_new_files_when_file_creation_is_not_permitted() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("edit.ts", "const value = 1;\n");

        let edit = make_diff("edit.ts", "const value = 1;", "const value = 2;");
        let create = make_diff("created.ts", "", "export const created = true;\n");
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.allow_file_create = false;
        orchestrator.diffs = vec![edit, create];

        let blocked = orchestrator.apply_diffs_to_fs().unwrap();

        assert_eq!(blocked, vec!["created.ts".to_string()]);
        // 编辑已有文件照常应用
        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(
            std::fs::read_to_string(env.root.join("edit.ts")).unwrap(),
            "const value = 2;\n"
        );
        // 新建文件既没写盘，也没被标记失败——它留待人工审查
        assert_eq!(orchestrator.diffs[1].status, "pending");
        assert!(!env.root.join("created.ts").exists());
    }

    #[test]
    fn auto_apply_creates_new_files_once_permitted() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();

        let create = make_diff("created.ts", "", "export const created = true;\n");
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.allow_file_create = true;
        orchestrator.diffs = vec![create];

        let blocked = orchestrator.apply_diffs_to_fs().unwrap();

        assert!(blocked.is_empty());
        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(
            std::fs::read_to_string(env.root.join("created.ts")).unwrap(),
            "export const created = true;\n"
        );
    }

    /// 今天靠人点界面验证的那条路径，现在是自动测试：逐 hunk 应用会推进文件
    /// 内容，第二个 hunk 必须仍能应用 —— 如果 restamp 没生效，它会因为
    /// baseHash 不匹配被误判为 stale。
    #[test]
    fn applying_hunks_one_by_one_keeps_the_rest_appliable() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        let first = orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();
        assert_eq!(first.applied.len(), 1, "failed: {:?}", first.failed);
        assert_eq!(orchestrator.diffs[0].status, "partial");
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::WaitingUser
        );

        let second = orchestrator.apply_diff_hunk(&diff_id, 1).unwrap();
        assert_eq!(
            second.applied.len(),
            1,
            "second hunk was rejected: {:?}",
            second.failed
        );

        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 2;\nconst second = 2;\n"
        );
        // 全部 hunk 落地后审查结束
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::Done
        );
    }

    #[test]
    fn apply_diff_hunk_rejects_unknown_ids_and_repeated_hunks() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("edit.ts", "const value = 1;\n");

        let diff = make_diff("edit.ts", "const value = 1;", "const value = 2;");
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        assert!(orchestrator
            .apply_diff_hunk("missing", 0)
            .unwrap_err()
            .contains("Diff not found"));
        assert!(orchestrator
            .apply_diff_hunk(&diff_id, 9)
            .unwrap_err()
            .contains("Hunk 9 not found"));

        orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();
        // 单 hunk 全部应用后整个 diff 变成 applied，此时不允许再次应用
        assert!(orchestrator
            .apply_diff_hunk(&diff_id, 0)
            .unwrap_err()
            .contains("cannot apply hunks while status is applied"));
    }

    /// 逐 hunk 审查过之后，整文件 Apply 仍然要能把剩下的落地。
    /// 旧实现只接受 `pending`，所以点过任意一个 hunk 之后整文件按钮就报
    /// "not pending"，用户只能一个个点完剩下的。
    #[test]
    fn apply_diff_finishes_a_partially_reviewed_file() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();
        assert_eq!(orchestrator.diffs[0].status, "partial");

        let rest = orchestrator.apply_diff(&diff_id).unwrap();

        assert_eq!(rest.applied.len(), 1, "failed: {:?}", rest.failed);
        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 2;\nconst second = 2;\n"
        );
        // 没有剩余 hunk 时再点应该明确报错，而不是静默成功
        assert!(orchestrator
            .apply_diff(&diff_id)
            .unwrap_err()
            .contains("cannot be applied while status is applied"));
    }

    #[test]
    fn apply_diff_skips_hunks_that_were_already_rejected() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks[0].status = Some("rejected".to_string());
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        diff.status = "partial".to_string();
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.apply_diff(&diff_id).unwrap();

        // 被拒绝的 hunk 不能被整文件 Apply 重新写回去
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 1;\nconst second = 2;\n"
        );
        assert_eq!(
            orchestrator.diffs[0].hunks[0].status.as_deref(),
            Some("rejected")
        );
        assert_eq!(orchestrator.diffs[0].status, "partial");
    }

    /// 拒绝路径和应用路径共享同一套 hunk 状态收敛逻辑，这里覆盖它们交叉的场景：
    /// 先应用一个 hunk，再整文件 Reject 剩下的。旧实现要求 status == "pending"，
    /// 所以这一步会直接报 "not pending"。
    #[test]
    fn reject_diff_finishes_a_partially_applied_file() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();
        assert_eq!(orchestrator.diffs[0].status, "partial");

        let rejected = orchestrator.reject_diff(&diff_id).unwrap();

        // 已应用的 hunk 不能被整文件 Reject 反写成 rejected
        assert_eq!(rejected.hunks[0].status.as_deref(), Some("applied"));
        assert_eq!(rejected.hunks[1].status.as_deref(), Some("rejected"));
        // 整体状态由 hunk 推导：applied + rejected 混合 => partial，而不是硬写的 "rejected"
        assert_eq!(rejected.status, "partial");
        // 拒绝不写盘：第一个 hunk 的改动留着，第二个原样不动
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 2;\nconst second = 1;\n"
        );
        // 没有可拒绝的 hunk 时必须明确报错，而不是静默成功
        assert!(orchestrator
            .reject_diff(&diff_id)
            .unwrap_err()
            .contains("no hunks left to reject"));
    }

    #[test]
    fn reject_diff_hunk_converges_status_and_refuses_repeats() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];

        let first = orchestrator.reject_diff_hunk(&diff_id, 0).unwrap();
        assert_eq!(first.status, "partial");
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::WaitingUser
        );

        let second = orchestrator.reject_diff_hunk(&diff_id, 1).unwrap();
        assert_eq!(second.status, "rejected");
        // 全部决定完毕后不该继续挂在 WaitingUser
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::Done
        );

        assert!(orchestrator
            .reject_diff_hunk(&diff_id, 0)
            .unwrap_err()
            .contains("while status is rejected"));
        assert!(orchestrator
            .reject_diff_hunk("missing", 0)
            .unwrap_err()
            .contains("Diff not found"));
    }

    #[test]
    fn reject_diff_hunk_keeps_an_applied_hunk_untouched() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();

        assert!(orchestrator
            .reject_diff_hunk(&diff_id, 0)
            .unwrap_err()
            .contains("is already applied"));
        assert_eq!(
            orchestrator.diffs[0].hunks[0].status.as_deref(),
            Some("applied")
        );
    }

    /// Apply all 以前直接把 `self.diffs` 交给 `apply_pending_diffs`，而后者只处理
    /// `pending` —— 逐 hunk 审查过的文件会被整批静默跳过。
    #[test]
    fn apply_all_diffs_includes_partially_reviewed_files() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");
        env.write_file("other.ts", "const other = 1;\n");

        let mut multi = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        multi.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let multi_id = multi.id.clone();
        let other = make_diff("other.ts", "const other = 1;", "const other = 2;");
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![multi, other];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.apply_diff_hunk(&multi_id, 0).unwrap();
        assert_eq!(orchestrator.diffs[0].status, "partial");

        let result = orchestrator.apply_all_diffs();

        assert!(result.failed.is_empty(), "failed: {:?}", result.failed);
        // partial 的文件也被收尾，而不是被跳过
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 2;\nconst second = 2;\n"
        );
        assert_eq!(
            std::fs::read_to_string(env.root.join("other.ts")).unwrap(),
            "const other = 2;\n"
        );
        assert_eq!(orchestrator.diffs[0].status, "applied");
        assert_eq!(orchestrator.diffs[1].status, "applied");
        // hunk 级状态也要跟上，否则界面上的逐 hunk 标记会是空的
        assert!(orchestrator.diffs[0]
            .hunks
            .iter()
            .all(|hunk| hunk.status.as_deref() == Some("applied")));
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::Done
        );
    }

    #[test]
    fn apply_all_diffs_does_not_resurrect_rejected_hunks() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.reject_diff_hunk(&diff_id, 0).unwrap();
        let result = orchestrator.apply_all_diffs();

        assert!(result.failed.is_empty(), "failed: {:?}", result.failed);
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 1;\nconst second = 2;\n"
        );
        assert_eq!(orchestrator.diffs[0].status, "partial");
    }

    /// Reject all 以前只处理 `pending`，并且把所有历史 rejected diff 都算作本次
    /// 结果 —— action log 因此虚报条数。
    #[test]
    fn reject_all_diffs_reports_only_what_it_changed() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("a.ts", "const a = 1;\n");
        env.write_file("b.ts", "const b = 1;\n");

        let a = make_diff("a.ts", "const a = 1;", "const a = 2;");
        let a_id = a.id.clone();
        let b = make_diff("b.ts", "const b = 1;", "const b = 2;");
        let b_id = b.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![a, b];

        let first = orchestrator.reject_all_diffs();
        assert_eq!(first.len(), 2);

        // 第二轮新来一个 diff：只应报告这一个，而不是连上一轮的两个
        orchestrator
            .diffs
            .push(make_diff("c.ts", "const c = 1;", "const c = 2;"));
        let second = orchestrator.reject_all_diffs();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].file, "c.ts");

        assert!(orchestrator
            .diffs
            .iter()
            .all(|diff| diff.status == "rejected"));
        // 拒绝不写盘
        assert_eq!(
            std::fs::read_to_string(env.root.join("a.ts")).unwrap(),
            "const a = 1;\n"
        );
        assert!(orchestrator.diffs.iter().any(|diff| diff.id == a_id));
        assert!(orchestrator.diffs.iter().any(|diff| diff.id == b_id));
    }

    fn make_step(id: &str) -> TaskStep {
        TaskStep {
            id: id.to_string(),
            title: format!("Step {}", id),
            step_type: "code".to_string(),
            status: "todo".to_string(),
            logs: Vec::new(),
            scope: None,
            execution_mode: None,
        }
    }

    /// 一个只返回文字、没有产出 diff 的步骤以前会把状态硬置成 WaitingUser，
    /// 界面显示"需要处理"但审查区是空的，用户只能靠重跑脱身。
    /// 第一条真正驱动整条流水线的测试。以前写不出来：`run` 要 `AppHandle`。
    ///
    /// 断言的是"界面跟不跟得上"，而不只是返回值：前端的状态、计划、阶段全靠这些
    /// 事件，一次逻辑正确但一声不响的运行，在界面上和没跑过没有区别。
    #[test]
    fn a_run_announces_its_plan_and_pipeline_to_the_frontend() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("src/app.ts", "const value = 1;\n");

        let events = std::sync::Arc::new(crate::agent::events::RecordingEvents::new());
        let llm =
            crate::services::llm_client::LlmClient::new(crate::services::llm_client::LlmConfig {
                endpoint: "mock://orchestrator-test".to_string(),
                api_key: "sk-test".to_string(),
                model: "mock-model".to_string(),
                provider: "openai".to_string(),
                max_output_tokens: None,
                tool_call_mode: "text_protocol".to_string(),
                model_type: crate::services::llm_client::ModelType::from_string("openai"),
                local_model_config: None,
            });
        let orchestrator = tokio::sync::Mutex::new(AgentOrchestrator::new());

        let result = tokio::runtime::Runtime::new().unwrap().block_on(drive_run(
            &orchestrator,
            "update the greeting".to_string(),
            crate::services::context::AgentContext::new(env.root.to_string_lossy().as_ref()),
            ContextCompressionMode::Focused,
            None,
            crate::services::context::ContextSourceOptions {
                include_project_tree: false,
                include_git_diff: false,
                include_project_memory: false,
            },
            vec![crate::agent::multi_agent::PipelineStage::new(
                crate::agent::multi_agent::AgentRole::Coder,
                "Coder",
            )],
            IdeMode::Code,
            Arc::new(AtomicBool::new(false)),
            &llm,
            events.clone(),
        ));

        assert!(result.is_ok(), "{:?}", result);
        let names = events.names();
        // 计划出来了就要广播：Plan 面板只认这个事件
        assert!(
            names.iter().any(|name| name == "agent-plan-ready"),
            "{:?}",
            names
        );
        // 状态和阶段进度是另外两条独立的线，缺哪条界面就有一块不动
        assert!(
            names.iter().any(|name| name == "agent-state-changed"),
            "{:?}",
            names
        );
        assert!(
            names.iter().any(|name| name == "agent-pipeline-update"),
            "{:?}",
            names
        );
    }

    /// 检查一开始就全过时，修复循环不该问模型任何东西。
    ///
    /// 这条看着简单，但它是"自动修复"能不能默认开着的前提：每次运行结束都白跑
    /// 一轮模型，既费钱又会在通过的代码上乱改。
    /// 提示词契约：每个 stage 的请求里必须带着哪些东西。
    ///
    /// 提示词结构是能被重构悄悄改坏的 —— 某个角色的输出规则丢了、用户任务被挤掉、
    /// 上一阶段的结论没带上：运行照样 Ok，只有模型输出变差，而"变差"这个仓库里
    /// 没有任何测试能衡量。所以退一步，把可测的那部分钉住：请求的组成部分。
    ///
    /// 这也是 9.0.11（把 prose 拼接换成真正的消息线程）的安全网：那次重构会改动
    /// 每个 stage 的提示词结构，这条测试能立刻指出哪一部分在改完之后丢了。
    #[test]
    fn every_stage_request_carries_the_task_role_rules_and_prior_work() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("src/app.ts", "const value = 1;\n");

        let recorder = std::sync::Arc::new(crate::services::llm_client::RequestRecorder::new());
        let events = std::sync::Arc::new(crate::agent::events::RecordingEvents::new());
        let llm =
            crate::services::llm_client::LlmClient::new(crate::services::llm_client::LlmConfig {
                endpoint: "mock://prompt-contract".to_string(),
                api_key: "sk-test".to_string(),
                model: "mock-model".to_string(),
                provider: "openai".to_string(),
                max_output_tokens: None,
                tool_call_mode: "text_protocol".to_string(),
                model_type: crate::services::llm_client::ModelType::from_string("openai"),
                local_model_config: None,
            })
            .with_request_recorder(recorder.clone());
        let orchestrator = tokio::sync::Mutex::new(AgentOrchestrator::new());

        let result = tokio::runtime::Runtime::new().unwrap().block_on(drive_run(
            &orchestrator,
            "rename the greeting helper".to_string(),
            crate::services::context::AgentContext::new(env.root.to_string_lossy().as_ref()),
            ContextCompressionMode::Focused,
            None,
            crate::services::context::ContextSourceOptions {
                include_project_tree: false,
                include_git_diff: false,
                include_project_memory: false,
            },
            vec![
                crate::agent::multi_agent::PipelineStage::new(
                    crate::agent::multi_agent::AgentRole::Architect,
                    "Architect",
                ),
                crate::agent::multi_agent::PipelineStage::new(
                    crate::agent::multi_agent::AgentRole::Coder,
                    "Coder",
                ),
            ],
            IdeMode::Code,
            Arc::new(AtomicBool::new(false)),
            &llm,
            events.clone(),
        ));
        assert!(result.is_ok(), "{:?}", result);

        let stage_messages = |stage: &str| -> Vec<crate::services::llm_client::ChatMessage> {
            recorder
                .requests()
                .into_iter()
                .find(|messages| {
                    messages.iter().any(|message| {
                        message
                            .content
                            .contains(&format!("Pipeline stage: {}", stage))
                    })
                })
                .unwrap_or_else(|| panic!("没有找到 {} 阶段的请求", stage))
        };
        let joined = |messages: &[crate::services::llm_client::ChatMessage]| -> String {
            messages
                .iter()
                .map(|message| message.content.clone())
                .collect::<Vec<_>>()
                .join("\n")
        };

        let architect = joined(&stage_messages("Architect"));
        // 用户任务必须原样出现：这是每个 stage 唯一的目标来源
        assert!(
            architect.contains("rename the greeting helper"),
            "{}",
            architect
        );
        // 角色的输出规则也必须在：丢了它 Architect 就会开始输出 diff
        assert!(
            architect.contains("Do not output code diffs"),
            "{}",
            architect
        );
        // 审查区现状要带上，否则 stage 会重复提议已经存在的改动
        assert!(architect.contains("pending diffs"), "{}", architect);

        let coder = stage_messages("Coder");
        let coder_text = joined(&coder);
        assert!(
            coder_text.contains("rename the greeting helper"),
            "{}",
            coder_text
        );
        // 上一阶段的产出必须传下去，而且是以真实 assistant 消息的形式 ——
        // 不是被拼进 user 消息的一段 prose。这正是 9.0.11 换掉的东西：
        // 只有真实消息才能同时把工具调用和工具结果带过来。
        let architect_roles: Vec<&str> = coder
            .iter()
            .filter(|message| message.content.contains("[Architect"))
            .map(|message| message.role.as_str())
            .collect();
        assert_eq!(architect_roles, vec!["assistant"], "{}", coder_text);
        // planner 的结论同样在线程里，并且带着出处标签
        assert!(
            coder
                .iter()
                .any(|message| message.role == "assistant"
                    && message.content.contains("[Planner]")),
            "{}",
            coder_text
        );
    }

    /// 续跑必须把暂停前工具**实际返回的内容**带回请求里。
    ///
    /// 在此之前跨阶段只传扁平文本，而扁平文本里从来没有工具返回值 ——
    /// 下一个 stage 只能看到模型对"我跑了测试"的转述，而转述正是最不该被信任的部分。
    /// 顺带钉住协议约束：`tool` 消息前面必须紧跟发起它的 assistant 调用，
    /// 少了配对整个请求会被供应商拒掉。
    #[test]
    fn a_resumed_run_still_shows_the_model_what_the_tools_returned() {
        let _guard = workspace::env_test_guard();
        let _env = TestEnv::new();

        let recorder = std::sync::Arc::new(crate::services::llm_client::RequestRecorder::new());
        let events = std::sync::Arc::new(crate::agent::events::RecordingEvents::new());
        let llm =
            crate::services::llm_client::LlmClient::new(crate::services::llm_client::LlmConfig {
                endpoint: "mock://resumed-thread".to_string(),
                api_key: "sk-test".to_string(),
                model: "mock-model".to_string(),
                provider: "openai".to_string(),
                max_output_tokens: None,
                tool_call_mode: "text_protocol".to_string(),
                model_type: crate::services::llm_client::ModelType::from_string("openai"),
                local_model_config: None,
            })
            .with_request_recorder(recorder.clone());
        let orchestrator = tokio::sync::Mutex::new(AgentOrchestrator::new());

        let transcript = vec![
            crate::services::llm_client::ChatMessage::assistant_tool_calls(
                "checking the tests".to_string(),
                &[crate::services::llm_client::LlmToolCall {
                    id: "call-1".to_string(),
                    name: "workspace_run_command".to_string(),
                    arguments: "{\"command\":\"npm test\"}".to_string(),
                }],
            ),
            crate::services::llm_client::ChatMessage::tool_result(
                "call-1",
                "npm test: 1 failing in src/greet.test.ts".to_string(),
            ),
        ];

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(drive_pipeline(
                &orchestrator,
                PipelineRun {
                    prompt: "fix the failing test".to_string(),
                    ctx_str: "project context".to_string(),
                    context_summary: "summary".to_string(),
                    pipeline: vec![crate::agent::multi_agent::PipelineStage::new(
                        crate::agent::multi_agent::AgentRole::Coder,
                        "Coder",
                    )],
                    transcript,
                    ide_mode: IdeMode::Code,
                },
                0,
                true,
                Arc::new(AtomicBool::new(false)),
                &llm,
                events.clone(),
            ));
        assert!(result.is_ok(), "{:?}", result);

        let request = recorder
            .requests()
            .into_iter()
            .next()
            .expect("续跑至少要发出一次请求");
        let tool_index = request
            .iter()
            .position(|message| message.role == "tool")
            .unwrap_or_else(|| panic!("请求里没有 tool 消息: {:?}", request));
        assert!(
            request[tool_index].content.contains("1 failing"),
            "{:?}",
            request[tool_index]
        );
        assert!(
            request[tool_index - 1].tool_calls.is_some(),
            "tool 结果前面必须是发起它的 assistant 调用: {:?}",
            request[tool_index - 1]
        );
    }

    #[test]
    fn repair_loop_does_not_call_the_model_when_checks_already_pass() {
        let _guard = workspace::env_test_guard();
        let _env = TestEnv::new();

        let events = std::sync::Arc::new(crate::agent::events::RecordingEvents::new());
        // endpoint 故意指向一个不存在的地址：真去调用就会失败，测试也就失败
        let llm =
            crate::services::llm_client::LlmClient::new(crate::services::llm_client::LlmConfig {
                endpoint: "http://127.0.0.1:1/never-called".to_string(),
                api_key: "sk-test".to_string(),
                model: "unused".to_string(),
                provider: "openai".to_string(),
                max_output_tokens: None,
                tool_call_mode: "text_protocol".to_string(),
                model_type: crate::services::llm_client::ModelType::from_string("openai"),
                local_model_config: None,
            });
        let orchestrator = tokio::sync::Mutex::new(AgentOrchestrator::new());

        let outcome = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(drive_repair(
                &orchestrator,
                "keep the build green".to_string(),
                vec!["cargo --version".to_string()],
                crate::services::verification::RepairPolicy::new(2, true),
                Arc::new(AtomicBool::new(false)),
                &llm,
                events.clone(),
            ))
            .expect("repair loop");

        assert_eq!(outcome.iterations, 0);
        assert_eq!(
            outcome.stop,
            crate::services::verification::RepairStop::ChecksPassed
        );
        assert!(!outcome.checks_failed);
        // 没修过就不该往 action log 里写"停下来了"，那是纯噪音
        assert_eq!(events.count("agent-action-log"), 0);
    }

    /// 修不好的时候：预算用完就停，并且每一轮都留下记录。
    #[test]
    fn repair_loop_gives_up_and_records_each_iteration() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("src/app.ts", "const value = 1;\n");

        let events = std::sync::Arc::new(crate::agent::events::RecordingEvents::new());
        let llm =
            crate::services::llm_client::LlmClient::new(crate::services::llm_client::LlmConfig {
                endpoint: "mock://repair-loop".to_string(),
                api_key: "sk-test".to_string(),
                model: "mock-model".to_string(),
                provider: "openai".to_string(),
                max_output_tokens: None,
                tool_call_mode: "text_protocol".to_string(),
                model_type: crate::services::llm_client::ModelType::from_string("openai"),
                local_model_config: None,
            });
        // mock 不会让这条命令通过，所以循环一定会用完预算
        let check = if cfg!(windows) {
            "findstr never-appears src/app.ts"
        } else {
            "grep never-appears src/app.ts"
        };
        let orchestrator = tokio::sync::Mutex::new(AgentOrchestrator::new());

        let outcome = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(drive_repair(
                &orchestrator,
                "make the check pass".to_string(),
                vec![check.to_string()],
                crate::services::verification::RepairPolicy::new(2, true),
                Arc::new(AtomicBool::new(false)),
                &llm,
                events.clone(),
            ))
            .expect("repair loop");

        assert!(outcome.iterations >= 1, "至少修过一轮");
        assert!(outcome.iterations <= 2, "不能超过预算");
        assert!(outcome.checks_failed, "检查始终没过");
        assert_ne!(
            outcome.stop,
            crate::services::verification::RepairStop::ChecksPassed
        );
        // 每一轮 + 最后的停止说明都要进 action log，否则用户只看到工作区变了
        // 却不知道 Agent 试了几次、为什么放弃
        assert!(
            events.count("agent-action-log") > outcome.iterations as usize,
            "iterations={} logs={}",
            outcome.iterations,
            events.count("agent-action-log")
        );
    }

    #[test]
    fn a_step_without_diffs_does_not_park_the_ui_in_waiting_user() {
        let _guard = workspace::env_test_guard();
        let _env = TestEnv::new();

        let step = make_step("s1");
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.begin_step(&step, "started");
        assert_eq!(orchestrator.steps[0].status, "doing");

        let outcome =
            orchestrator.record_step_success(&step, "s1 done, nothing to change.", None, None);

        assert_eq!(outcome.new_diffs, 0);
        assert_eq!(outcome.step.status, "done");
        assert_eq!(orchestrator.steps.len(), 1, "步骤应被就地更新而不是追加");
        assert_eq!(orchestrator.steps[0].status, "done");
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::Done
        );
    }

    #[test]
    fn a_step_with_diffs_stamps_base_hashes_and_waits_for_review() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("src/app.ts", "const value = 1;\n");

        let step = make_step("s1");
        let mut orchestrator = AgentOrchestrator::new();
        // 上一轮遗留的一个 diff：new_diffs 只应统计本次新增的
        orchestrator
            .diffs
            .push(make_diff("old.ts", "const old = 1;", "const old = 2;"));

        let response = "```diff:src/app.ts\n<<<<<<< ORIGINAL\nconst value = 1;\n=======\nconst value = 2;\n>>>>>>> UPDATED\n```";
        let outcome = orchestrator.record_step_success(&step, response, None, None);

        assert_eq!(outcome.new_diffs, 1);
        assert_eq!(orchestrator.diffs.len(), 2);
        // baseHash 必须在生成时盖章，否则 apply 时无法识别期间的外部改动
        assert!(orchestrator.diffs[1].base_hash.is_some());
        assert_eq!(
            orchestrator.diffs[1]
                .provenance
                .as_ref()
                .unwrap()
                .source_stage,
            Some("Step s1".to_string())
        );
        assert_eq!(
            orchestrator.state_mgr.state,
            crate::agent::state_machine::AgentState::WaitingUser
        );
    }

    /// 单次运行上限只要中途暂停一次就归零重算的话就形同虚设，所以
    /// `begin_run` 不能清掉记账器，`resumed_usage_meter` 必须返回同一个实例。
    #[test]
    fn token_accounting_survives_a_pause_and_resume() {
        let meter = std::sync::Arc::new(crate::services::llm_client::RunUsageMeter::new(Some(100)));
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.start_usage_accounting(meter.clone());

        meter.record_usage(Some(&crate::services::llm_client::LlmUsage {
            prompt_tokens: Some(60),
            completion_tokens: Some(10),
            total_tokens: None,
        }));

        // 恢复时 run id 会重新写入，但额度必须接着算
        orchestrator.begin_run(Some("run-1".to_string()));
        let resumed = orchestrator
            .resumed_usage_meter()
            .expect("paused run should hand its meter back");

        assert_eq!(resumed.snapshot().total_tokens, 70);
        resumed.record_usage(Some(&crate::services::llm_client::LlmUsage {
            prompt_tokens: Some(40),
            completion_tokens: None,
            total_tokens: None,
        }));
        // 续跑的消耗算在同一个额度里，因此这里已经越线
        assert!(resumed.check_budget().is_err());
        assert_eq!(meter.snapshot().total_tokens, 110);
    }

    /// 每次运行原本都是冷启动，跟进一句"再处理下错误分支"读不到上一轮做了什么。
    #[test]
    fn conversation_turns_carry_forward_and_stay_bounded() {
        let mut orchestrator = AgentOrchestrator::new();
        assert!(orchestrator.conversation_digest().is_none());

        orchestrator
            .diffs
            .push(make_diff("src/app.ts", "const a = 1;", "const a = 2;"));
        orchestrator.record_conversation_turn("rename the value");

        let digest = orchestrator.conversation_digest().expect("digest");
        assert!(digest.contains("rename the value"), "{}", digest);
        // 结果里要说清上一轮留下了什么，否则"接着上一句"仍然无从下手
        assert!(digest.contains("src/app.ts"), "{}", digest);
        assert!(digest.contains("awaiting review"), "{}", digest);

        // 只保留末尾若干轮：早期轮次对"接着上一句"没帮助，却一直占预算
        for index in 0..MAX_CONVERSATION_TURNS * 2 {
            orchestrator.record_conversation_turn(&format!("turn {}", index));
        }
        assert_eq!(orchestrator.conversation.len(), MAX_CONVERSATION_TURNS);
        let digest = orchestrator.conversation_digest().expect("digest");
        assert!(!digest.contains("rename the value"), "{}", digest);

        orchestrator.clear_conversation();
        assert!(orchestrator.conversation_digest().is_none());
    }

    #[test]
    fn a_run_without_changes_is_recorded_as_such() {
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.record_conversation_turn("explain the build pipeline");

        let digest = orchestrator.conversation_digest().expect("digest");
        // 不能记成"改了文件"，否则下一轮模型会以为已经动过代码
        assert!(digest.contains("no file changes produced"), "{}", digest);
    }

    /// 应用之前是单向的：落盘之后审查界面就无能为力了，而"应用了才发现不对"
    /// 恰恰最需要退路。撤销后还必须能重新应用，否则只是把问题换了个形状。
    #[test]
    fn undo_restores_the_file_and_lets_the_diff_be_applied_again() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("multi.ts", "const first = 1;\nconst second = 1;\n");

        let mut diff = make_diff("multi.ts", "const first = 1;", "const first = 2;");
        diff.hunks.push(crate::agent::state_machine::DiffHunk {
            old_start: 2,
            old_lines: 1,
            new_start: 2,
            new_lines: 1,
            content: String::new(),
            original: "const second = 1;".to_string(),
            updated: "const second = 2;".to_string(),
            provenance: None,
            status: None,
        });
        let diff_id = diff.id.clone();
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![diff];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 2;\nconst second = 1;\n"
        );
        let (label, files) = orchestrator.pending_undo().expect("undo available");
        assert!(label.contains("Apply hunk 1"), "{}", label);
        assert_eq!(files, vec!["multi.ts".to_string()]);

        let undone = orchestrator.undo_last_apply().unwrap();

        assert_eq!(undone.restored, vec!["multi.ts".to_string()]);
        assert!(undone.failed.is_empty());
        assert_eq!(
            std::fs::read_to_string(env.root.join("multi.ts")).unwrap(),
            "const first = 1;\nconst second = 1;\n"
        );
        // 改动退回审查区，而不是留在"已应用"却和磁盘不一致
        assert_eq!(orchestrator.diffs[0].status, "pending");
        assert_eq!(orchestrator.diffs[0].hunks[0].status, None);

        // baseHash 必须跟着回退，否则重新应用会被误判 stale
        let result = orchestrator.apply_diff_hunk(&diff_id, 0).unwrap();
        assert_eq!(result.applied.len(), 1, "failed: {:?}", result.failed);

        // 栈已空
        assert!(orchestrator.pending_undo().is_some());
        orchestrator.undo_last_apply().unwrap();
        assert!(orchestrator.pending_undo().is_none());
        assert!(orchestrator
            .undo_last_apply()
            .unwrap_err()
            .contains("Nothing to undo"));
    }

    #[test]
    fn undoing_a_created_file_removes_it() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();

        let create = make_diff("created.ts", "", "export const created = true;\n");
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.allow_file_create = true;
        orchestrator.diffs = vec![create];

        orchestrator.apply_diffs_to_fs().unwrap();
        assert!(env.root.join("created.ts").exists());

        let undone = orchestrator.undo_last_apply().unwrap();

        assert_eq!(undone.restored, vec!["created.ts".to_string()]);
        // 原本不存在的文件，撤销就是删掉它，而不是留下一个空文件
        assert!(!env.root.join("created.ts").exists());
    }

    /// 自动应用里"先压回滚点、再翻状态"的顺序是有约束的，不是随手写的。
    ///
    /// 一批 diff 里只要有一条失败，`apply_diffs_to_fs` 就返回 `Err` —— 但成功的那几条
    /// 已经落盘了。回滚点在返回之前就必须存在，否则一次部分失败会让已经改掉的文件
    /// 失去唯一的退路。这也是把两步拆到不同临界区最危险的地方：中间插进来的
    /// `undo_last_apply` 会弹掉这个 checkpoint，而 diff 还写着 pending。
    #[test]
    fn a_partly_failed_auto_apply_still_leaves_a_way_back() {
        let _guard = workspace::env_test_guard();
        let env = TestEnv::new();
        env.write_file("lands.ts", "const value = 1;\n");
        env.write_file("misses.ts", "const other = 1;\n");

        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![
            make_diff("lands.ts", "const value = 1;", "const value = 2;"),
            // 引用了文件里不存在的内容：这一条必然应用失败
            make_diff("misses.ts", "const missing = 9;", "const missing = 10;"),
        ];
        crate::agent::diff_apply::stamp_base_hashes(&mut orchestrator.diffs);

        let result = orchestrator.apply_diffs_to_fs();

        assert!(result.is_err(), "有一条失败时应当返回 Err: {:?}", result);
        // 成功的那条确实落盘了
        assert_eq!(
            std::fs::read_to_string(env.root.join("lands.ts")).unwrap(),
            "const value = 2;\n"
        );
        // 而且它有退路 —— 这是这条测试的全部意义
        let (_, files) = orchestrator
            .pending_undo()
            .expect("部分失败也必须留下回滚点");
        assert!(
            files.iter().any(|file| file.contains("lands.ts")),
            "回滚点要覆盖已经落盘的文件: {:?}",
            files
        );

        // 撤销之后内容回到应用之前
        orchestrator.undo_last_apply().expect("undo");
        assert_eq!(
            std::fs::read_to_string(env.root.join("lands.ts")).unwrap(),
            "const value = 1;\n"
        );
    }

    #[test]
    fn summarize_pending_diffs_includes_actual_reviewable_diff_context() {
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.diffs = vec![
            make_diff("src/app.ts", "const oldValue = 1;", "const newValue = 2;"),
            FileDiff {
                status: "partial".to_string(),
                ..make_diff("src/partial.ts", "partial(false)", "partial(true)")
            },
            FileDiff {
                status: "applied".to_string(),
                ..make_diff("src/done.ts", "done()", "done(true)")
            },
        ];

        let summary = orchestrator.summarize_pending_diffs();

        assert!(summary.contains("Reviewable diffs: 2"));
        assert!(summary.contains("src/app.ts"));
        assert!(summary.contains("src/partial.ts [partial]"));
        assert!(summary.contains("Original excerpt: const oldValue = 1;"));
        assert!(summary.contains("Updated excerpt: const newValue = 2;"));
        assert!(!summary.contains("src/done.ts"));
    }

    #[test]
    fn attach_stage_provenance_records_role_and_stage() {
        let mut diffs = vec![make_diff("src/app.ts", "old", "new")];

        attach_stage_provenance(&mut diffs, "coder", "Implement");

        let provenance = diffs[0].provenance.as_ref().expect("provenance");
        assert_eq!(provenance.source_role.as_deref(), Some("coder"));
        assert_eq!(provenance.source_stage.as_deref(), Some("Implement"));
    }

    #[test]
    fn run_id_tracks_current_and_last_run() {
        let mut orchestrator = AgentOrchestrator::new();

        let lease = orchestrator
            .try_begin_run(Some("run-1".to_string()))
            .expect("空闲时应当抢到执行权");

        assert_eq!(orchestrator.current_run_id.as_deref(), Some("run-1"));
        assert_eq!(orchestrator.last_run_id.as_deref(), Some("run-1"));

        orchestrator.finish_run(lease.claim);

        assert_eq!(orchestrator.current_run_id, None);
        assert_eq!(orchestrator.last_run_id.as_deref(), Some("run-1"));
    }

    /// 状态栏要一直显示这次运行花了多少，所以用量搭 `agent-state-changed` 的车。
    ///
    /// 关键是"算不出来"和"没花钱"必须分得开：没配价格时 `spendMicros` 是 null，
    /// 打印成 0 会让一次真花了钱的运行显示成免费。
    #[test]
    fn state_payload_carries_run_usage_without_faking_a_price() {
        use crate::services::llm_client::{LlmUsage, RunUsageMeter, TokenPricing};

        let mut orchestrator = AgentOrchestrator::new();
        assert!(
            orchestrator.state_payload()["usage"].is_null(),
            "没有记账器时不该编一份用量出来"
        );

        let priced = Arc::new(RunUsageMeter::new(Some(10_000)).with_spend_cap(
            Some(TokenPricing {
                prompt_micros_per_million: 1_000_000,
                completion_micros_per_million: 2_000_000,
            }),
            Some(500_000),
        ));
        priced.record_usage(Some(&LlmUsage {
            prompt_tokens: Some(1_000),
            completion_tokens: Some(500),
            total_tokens: None,
        }));
        orchestrator.start_usage_accounting(priced);

        let usage = orchestrator.state_payload()["usage"].clone();
        assert_eq!(usage["totalTokens"], 1_500);
        assert_eq!(usage["maxTotalTokens"], 10_000);
        // 1000 * $1/M + 500 * $2/M = $0.002
        assert_eq!(usage["spendMicros"], 2_000);
        assert_eq!(usage["maxSpendMicros"], 500_000);
        assert_eq!(usage["reportedCalls"], 1);

        let unpriced = Arc::new(RunUsageMeter::new(None));
        unpriced.record_usage(Some(&LlmUsage {
            prompt_tokens: Some(1_000),
            completion_tokens: Some(500),
            total_tokens: None,
        }));
        orchestrator.start_usage_accounting(unpriced);

        let usage = orchestrator.state_payload()["usage"].clone();
        assert_eq!(usage["totalTokens"], 1_500);
        assert!(
            usage["spendMicros"].is_null(),
            "没配价格就是算不出来，不是花了 0"
        );
        assert!(usage["maxTotalTokens"].is_null());
    }

    /// 同一时刻只能有一个运行。
    ///
    /// 这条约束以前是命令层整段持锁**顺带**保证的；锁收窄到每阶段之后就必须
    /// 自己守住，否则第二个 prompt 会把第一个的计划和工具面覆盖掉。
    #[test]
    fn a_second_run_is_refused_while_one_is_in_flight() {
        let mut orchestrator = AgentOrchestrator::new();

        let first = orchestrator
            .try_begin_run(Some("run-1".to_string()))
            .expect("第一个运行应当抢到执行权");

        let second = orchestrator.try_begin_run(Some("run-2".to_string()));
        assert!(second.is_err(), "并发的第二个运行必须被拒绝");
        // 被拒绝不能顺手改掉在跑那个运行的身份
        assert_eq!(orchestrator.current_run_id.as_deref(), Some("run-1"));

        orchestrator.finish_run(first.claim);
        let reused = orchestrator
            .try_begin_run(Some("run-2".to_string()))
            .expect("上一个运行结束后应当能再开一个");
        orchestrator.finish_run(reused.claim);
    }

    /// 只有持有者能交还执行权，且 Stop 之后的新运行不会把旧运行"取消解除"。
    ///
    /// 被 Stop 放弃的旧运行会在几分钟后从一个不可中断的工具调用里醒来收尾。那时
    /// 执行权可能已经属于一个新运行 —— 第一版的 `finish_run` 无条件清标志，于是
    /// 旧运行的收尾会把新运行的执行权一起放掉，第三个 prompt 就能在新运行还在跑
    /// 的时候进来，正好是这个机制要挡的事。
    ///
    /// 取消开关同理：共享一个 `Arc` 时，新运行启动前的那句"清掉上次的取消状态"
    /// 会把还在排空的旧运行**复活**。一次运行一个开关之后这件事不可能发生。
    #[test]
    fn a_stale_run_cannot_release_someone_elses_claim() {
        let mut orchestrator = AgentOrchestrator::new();

        let stopped = orchestrator
            .try_begin_run(Some("run-1".to_string()))
            .expect("第一个运行应当抢到执行权");
        // 用户点了 Stop：执行权被强行收回，但 run-1 还在某个工具调用里跑着
        orchestrator.abandon_run();
        assert!(
            stopped.cancel.load(Ordering::SeqCst),
            "Stop 必须把那次运行自己的取消开关拉下来"
        );

        let fresh = orchestrator
            .try_begin_run(Some("run-2".to_string()))
            .expect("Stop 之后应当能开新运行");
        assert!(
            stopped.cancel.load(Ordering::SeqCst),
            "新运行不该把被停掉那次的取消状态解除"
        );
        assert!(!fresh.cancel.load(Ordering::SeqCst), "新运行自己不该是取消态");

        // run-1 终于醒来收尾
        orchestrator.finish_run(stopped.claim);

        let third = orchestrator.try_begin_run(Some("run-3".to_string()));
        assert!(
            third.is_err(),
            "run-2 还在跑，它的执行权不该被 run-1 的收尾放掉"
        );
        assert_eq!(orchestrator.current_run_id.as_deref(), Some("run-2"));

        orchestrator.finish_run(fresh.claim);
        assert_eq!(orchestrator.current_run_id, None);
    }

    /// 漏掉 `finish_run` 不该把应用锁死。
    ///
    /// 执行权的释放本来全靠调用方在每条退出路径上记得调 `finish_run` —— 而这个
    /// 会话里我在相邻两个改动里各漏过一次同类的生命周期管理。所以 lease 带一个
    /// 存活凭证，orchestrator 只留 `Weak`：lease 一被丢弃（提前返回、panic），
    /// 下一次抢占就把这份没人持有的执行权回收掉，而不是永久拒绝所有后续运行。
    #[test]
    fn a_dropped_lease_releases_the_slot_even_without_finish_run() {
        let mut orchestrator = AgentOrchestrator::new();

        {
            let leaked = orchestrator
                .try_begin_run(Some("run-1".to_string()))
                .expect("空闲时应当抢到执行权");
            assert!(
                orchestrator.try_begin_run(Some("run-2".to_string())).is_err(),
                "凭证还活着时必须拒绝第二个运行"
            );
            drop(leaked);
        }

        let recovered = orchestrator
            .try_begin_run(Some("run-2".to_string()))
            .expect("lease 已经没人持有，执行权应当被回收");
        assert_eq!(orchestrator.current_run_id.as_deref(), Some("run-2"));

        // 迟到的旧收尾仍然不能动新运行的执行权
        orchestrator.finish_run(RunClaim(0));
        assert_eq!(orchestrator.current_run_id.as_deref(), Some("run-2"));
        orchestrator.finish_run(recovered.claim);
        assert_eq!(orchestrator.current_run_id, None);
    }

    /// 把 `cancel` 移走之后，执行权仍然属于这次运行。
    ///
    /// 四个入口都是这么用的：`let claim = lease.claim;` 然后把 `lease.cancel` 交给
    /// 驱动器。部分移动不会顺带丢掉 `_alive`，所以执行权活到函数结束 —— 但这条
    /// 性质是四处调用点默默依赖的，而它靠的是"部分移动只移动那一个字段"这个语言
    /// 细节。哪天有人把 lease 解构掉，执行权会在运行还在跑的时候就被回收。
    #[test]
    fn moving_the_cancel_switch_out_does_not_release_the_slot() {
        let mut orchestrator = AgentOrchestrator::new();

        let lease = orchestrator
            .try_begin_run(Some("run-1".to_string()))
            .expect("空闲时应当抢到执行权");
        let claim = lease.claim;
        let _cancel = lease.cancel;

        assert!(
            orchestrator.try_begin_run(Some("run-2".to_string())).is_err(),
            "开关被移走不代表这次运行结束了"
        );

        orchestrator.finish_run(claim);
        let next = orchestrator
            .try_begin_run(Some("run-2".to_string()))
            .expect("正常收尾之后应当能再开一个");
        orchestrator.finish_run(next.claim);
    }
    ///
    /// `repair_workspace` 会跨 await 一直持着那把锁。第一版把开关只放在
    /// orchestrator 里，于是 Stop 得先抢锁 —— 只能干等到修复自己结束，而那时开关
    /// 已经交回去了，拉了个空：Stop 退化成一个只重置界面的空动作，Auto 模式下
    /// 修复循环还在往磁盘上写。所以句柄要单独发布出来。
    #[test]
    fn the_registry_cancels_without_the_orchestrator_lock() {
        let mut orchestrator = AgentOrchestrator::new();
        let registry = orchestrator.cancel_registry();

        // 空闲时拉开关是无操作，不该 panic
        registry.cancel_active_run();

        let lease = orchestrator
            .try_begin_run(Some("run-1".to_string()))
            .expect("空闲时应当抢到执行权");
        assert!(!lease.cancel.load(Ordering::SeqCst));

        // 关键：这里没有碰 orchestrator
        registry.cancel_active_run();
        assert!(
            lease.cancel.load(Ordering::SeqCst),
            "句柄必须能直接拉到当前运行的开关"
        );

        orchestrator.finish_run(lease.claim);
        let next = orchestrator
            .try_begin_run(Some("run-2".to_string()))
            .expect("上一个运行结束后应当能再开一个");
        assert!(
            !next.cancel.load(Ordering::SeqCst),
            "上一次的取消不该沾到新运行头上"
        );
        // 交回之后句柄里已经没有开关了，再拉一次不该影响任何人
        orchestrator.finish_run(next.claim);
        registry.cancel_active_run();
        assert!(!next.cancel.load(Ordering::SeqCst));
    }

    /// 阶段执行期间用户点了 Stop：结果不能再往一份已经不存在的计划里落地。
    ///
    /// `stop_agent` 会清空 `steps`。驱动器在模型调用期间不持锁，所以这件事真的
    /// 会发生 —— 按下标写回去就是越界 panic，把整个后端带走。
    #[test]
    fn a_stage_finishing_after_stop_lands_nothing() {
        let _guard = workspace::env_test_guard();
        let _env = TestEnv::new();

        let events = crate::agent::events::RecordingEvents::new();
        let mut orchestrator = AgentOrchestrator::new();
        let mut run = PipelineRun {
            prompt: "fix the bug".to_string(),
            ctx_str: "context".to_string(),
            context_summary: "summary".to_string(),
            pipeline: vec![crate::agent::multi_agent::PipelineStage::new(
                crate::agent::multi_agent::AgentRole::Coder,
                "Coder",
            )],
            transcript: Vec::new(),
            ide_mode: IdeMode::Code,
        };

        let StagePlan::Ready { stage, step_id, .. } =
            orchestrator.prepare_stage(&mut run, 0, false, &events)
        else {
            panic!("这个阶段没有配置 pause_before，应当可以执行");
        };

        // 用户按了 Stop
        orchestrator.steps.clear();

        let landed = orchestrator.record_stage_outcome(
            &mut run,
            0,
            &stage,
            &step_id,
            Ok(executor::StageOutcome {
                text: "```diff\n--- a/src/app.ts\n+++ b/src/app.ts\n```".to_string(),
                transcript: vec![crate::services::llm_client::ChatMessage::assistant(
                    "done".to_string(),
                )],
            }),
            &Arc::new(AtomicBool::new(false)),
            &events,
        );

        assert_eq!(landed.err().as_deref(), Some(CANCELLED_ERROR));
        assert!(orchestrator.diffs.is_empty(), "被停掉的阶段不该留下 diff");
        assert!(run.transcript.is_empty(), "被停掉的阶段不该续写线程");
    }

    /// 暂停快照要能原样恢复历史。恢复的是真实消息线程而不是扁平文本：
    /// 续跑之后的 stage 必须还能看到暂停前工具到底返回了什么。
    #[test]
    fn paused_pipeline_snapshot_can_be_stored_for_resume() {
        let mut orchestrator = AgentOrchestrator::new();
        let pipeline = crate::agent::multi_agent::default_pipeline();

        orchestrator.paused_run = Some(PausedPipelineRun {
            prompt: "Fix issue".to_string(),
            context: "context".to_string(),
            context_summary: "summary".to_string(),
            transcript: vec![
                crate::services::llm_client::ChatMessage::assistant("[Planner]\nstep one"),
                crate::services::llm_client::ChatMessage::tool_result(
                    "call-1",
                    "npm test: 1 failing".to_string(),
                ),
            ],
            pipeline: pipeline.clone(),
            stage_index: 1,
            ide_mode: IdeMode::Code,
        });

        let paused = orchestrator.paused_run.as_ref().expect("paused run");
        assert_eq!(paused.stage_index, 1);
        assert_eq!(paused.pipeline.len(), pipeline.len());
        assert!(paused.transcript[0].content.contains("step one"));
        // 工具结果本身要留在快照里，而不是只留模型对它的转述
        assert_eq!(paused.transcript[1].role, "tool");
        assert!(paused.transcript[1].content.contains("1 failing"));
    }

    #[test]
    fn format_context_sources_records_workspace_source_flags() {
        let summary = format_context_sources(&crate::services::context::ContextSourceOptions {
            include_project_tree: true,
            include_git_diff: false,
            include_project_memory: true,
        });

        assert_eq!(
            summary,
            "Context sources: projectTree=true, gitDiff=false, projectMemory=true"
        );
    }
}
