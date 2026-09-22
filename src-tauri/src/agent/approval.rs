//! 逐动作人工批准。
//!
//! 在这之前，后端**没有任何东西会等人**：MCP 的"批准策略"是运行开始前就定死的静态
//! 选择，审查区管的是"diff 已经存在之后要不要落盘"，而撤不回的外部动作（导航、截图、
//! 将来的输入注入）只有一道运行开始前给的会话级开关。会话级开关是这类动作的**下限**
//! 而不是上限：用户在运行开始时说"允许访问 localhost"，不等于同意此刻这一次导航。
//!
//! 这里提供那个缺掉的机制：一次工具调用可以在动手之前挂起，等前端把决定送回来。
//!
//! 三条不变量，都是"默认拒绝"的不同说法：
//! - **没人应答就是拒绝。** 超时、通道被丢弃、根本没装批准通道，一律不放行。批准必须
//!   是一个人做过的动作，不能是"没等到反对"。
//! - **Stop 拒绝一切挂起的请求。** 一个正在等批准的动作在 Stop 之后被批准，等于 Stop
//!   之后还发生了副作用。
//! - **停止等待就要通知前端关掉对话框。** 超时之后还开着的对话框会让用户点一个没人
//!   在等的"批准"，那比没有对话框更糟 —— 他会以为自己授权了什么。

use crate::agent::events::RunEvents;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

/// 请求批准时发给前端的事件名。载荷与前端 `DestructiveOpConfirm` 同构。
pub const APPROVAL_REQUESTED_EVENT: &str = "agent-approval-requested";
/// 模型问用户一个选择题时发给前端的事件名。
///
/// 和批准分成两个事件而不是给批准载荷加一个 `options` 字段：两者要的交互不同（批准是
/// 是/否，提问是从几个选项里挑一个或自己写一句），共用一个事件会让前端先猜是哪一种，
/// 猜错就是把一个选择题渲染成一次授权。
pub const QUESTION_REQUESTED_EVENT: &str = "agent-question-requested";
/// 后端已经不再等这条请求了（超时 / Stop / 已决定），前端该收掉对话框。
///
/// 批准和提问共用这一个：两边的等待都登记在同一张表里、用同一个 id，而"停止等待"这件事
/// 本身没有区别。前端按 id 对号，认不出的 id 直接忽略。
pub const APPROVAL_CLOSED_EVENT: &str = "agent-approval-closed";

/// 默认等人多久。
///
/// 有限而不是无限：一个永远挂着的工具调用会占着这次运行的执行权，用户看到的是
/// "Agent 卡住了"而不是"Agent 在等我"。两分钟够看清一条 URL 并做决定。
pub const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

/// 一次批准请求的内容。字段是给人看的，所以每一项都要能独立说明"将要发生什么"。
#[derive(Clone, Debug)]
pub struct ApprovalRequest {
    pub id: String,
    /// 动作类别，前端按它选图标和标签
    pub op_type: String,
    pub title: String,
    pub description: String,
    /// 具体到可以复盘的那一行（URL、命令、窗口标题）
    pub detail: String,
}

impl ApprovalRequest {
    /// id 由后端生成：它是等待方的钥匙，让前端回传一个自己编的 id 就等于让前端
    /// 决定自己在回答哪个问题。
    pub fn new(
        op_type: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            op_type: op_type.into(),
            title: title.into(),
            description: description.into(),
            detail: detail.into(),
        }
    }

    fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "opType": self.op_type,
            "title": self.title,
            "description": self.description,
            "detail": self.detail,
        })
    }
}

/// 等待的结果。五种，因为五种要写进记录的话不一样。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalOutcome {
    Approved,
    /// 人点了拒绝
    Denied,
    /// Stop 把挂起的请求拒掉了。
    ///
    /// 和 `Denied` 分开是因为记录要说真话：Stop 不是一个人对这次动作说不。混在一起的话
    /// 事后复盘看到的是"用户拒绝了这次导航"，而实际发生的是"用户停掉了整个运行"。
    /// 记录的类别也跟着不同（`*_cancelled` 而不是 `*_refused`），和 Stop 在工具入口
    /// 拦下调用时的写法一致。
    Cancelled,
    /// 没人在规定时间内应答
    TimedOut,
    /// 这次运行没有批准通道（headless 入口）。仍然是拒绝，只是原因不同 ——
    /// 记录里要能区分"用户不同意"和"根本没人可问"。
    Unattended,
}

impl ApprovalOutcome {
    /// 这次拒绝在记录里的类别后缀。
    ///
    /// Stop 拦下的动作用 `_cancelled`，其余用 `_refused` —— 沿用工具入口那道 Stop 闸门
    /// 已经在用的分类，否则同一件事在记录里有两种名字。
    pub fn record_suffix(&self) -> &'static str {
        match self {
            ApprovalOutcome::Cancelled => "_cancelled",
            _ => "_refused",
        }
    }

    /// 写进外部动作记录和工具返回值的那句话。`None` = 没被拒绝。
    ///
    /// 返回 `Option` 而不是给 `Approved` 也编一句话：一个"批准了"的字符串放在名字叫
    /// refusal 的函数里，早晚会被某个调用方写进一条拒绝记录。
    pub fn refusal_detail(&self) -> Option<&'static str> {
        match self {
            ApprovalOutcome::Approved => None,
            ApprovalOutcome::Denied => Some("The user denied this action."),
            ApprovalOutcome::Cancelled => {
                Some("This run was stopped, so the action was refused before it could take effect.")
            }
            ApprovalOutcome::TimedOut => {
                Some("Nobody approved this action in time, so it was not performed.")
            }
            ApprovalOutcome::Unattended => {
                Some("This run has no approval prompt attached, so the action was refused.")
            }
        }
    }
}

/// 一道给用户的选择题。
///
/// 和 `ApprovalRequest` 是两种东西：那个问"要不要让我做这件事"，这个问"你想要哪一种"。
/// 后者不是授权，模型拿到的是一个决定而不是一次许可。
#[derive(Clone, Debug)]
pub struct QuestionRequest {
    pub id: String,
    pub question: String,
    /// 模型给的候选项。前端另外永远提供一个"自己写"的入口 —— 只能在模型想到的几项里选，
    /// 等于让模型的想象力当成用户的全部选项。
    pub options: Vec<String>,
}

impl QuestionRequest {
    /// id 由后端生成，理由同 `ApprovalRequest::new`
    pub fn new(question: impl Into<String>, options: Vec<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            question: question.into(),
            options,
        }
    }

    fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "question": self.question,
            "options": self.options,
        })
    }
}

/// 问一道选择题的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuestionOutcome {
    /// 用户选了一项，或者自己写了一句。两者形状相同：区分"他挑的"和"他写的"对模型没有
    /// 用处，而把自由输入标成另一类会让模型去猜哪一种更可信。
    Answered(String),
    /// 没拿到回答。原因沿用 `ApprovalOutcome`，因为"他关掉了"、"Stop"、"超时"、
    /// "根本没人可问"这四件事在记录里和给模型的话里都不一样。
    Unanswered(ApprovalOutcome),
}

enum Decision {
    Approved,
    Denied,
    /// Stop。不是人做的决定，所以不能走 `Denied`。
    Cancelled,
    /// 用户对一道选择题给出的答案
    Answered(String),
}

/// 挂起的批准请求。
///
/// 只存 `oneshot::Sender`：等待方拿着 `Receiver`，所以"谁在等"这件事不需要第二份状态，
/// 而且发送方一旦被丢弃，等待方立刻得到 `Err` —— 通道本身就是拒绝的兜底。
#[derive(Clone, Default)]
pub struct ApprovalRegistry {
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Decision>>>>,
}

impl ApprovalRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 拿锁，中毒了也照用。
    ///
    /// `unwrap()` 会让一次 panic 变成整个进程的批准通道永久报废；`if let Ok` 更糟 ——
    /// 它会静默：此后每一次撤不回的动作都被拒，而用户看到的只是动作反复失败，没有一句
    /// 话说明批准通道已经坏了。临界区里只有 HashMap 的增删，本身不会 panic，所以恢复
    /// 使用是安全的。
    fn pending(&self) -> std::sync::MutexGuard<'_, HashMap<String, oneshot::Sender<Decision>>> {
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn register(&self, id: &str) -> oneshot::Receiver<Decision> {
        let (tx, rx) = oneshot::channel();
        self.pending().insert(id.to_string(), tx);
        rx
    }

    fn forget(&self, id: &str) {
        self.pending().remove(id);
    }

    /// 送回一个决定。返回是否真的有人在等这条请求 —— 前端可能在超时之后才点。
    pub fn resolve(&self, id: &str, approved: bool) -> bool {
        let sender = self.pending().remove(id);
        match sender {
            Some(sender) => sender
                .send(if approved {
                    Decision::Approved
                } else {
                    Decision::Denied
                })
                .is_ok(),
            None => false,
        }
    }

    /// 送回一道选择题的答案。返回是否真的有人在等 —— 同 `resolve`，超时之后才点到就是 false。
    ///
    /// 空答案不在这里挡：命令层已经拒掉了空串。让一个空字符串走到这里，模型会收到
    /// "用户选了 \"\""，那比没有答案更糟。
    pub fn answer(&self, id: &str, answer: impl Into<String>) -> bool {
        let sender = self.pending().remove(id);
        match sender {
            Some(sender) => sender.send(Decision::Answered(answer.into())).is_ok(),
            None => false,
        }
    }

    /// 拒掉所有挂起的请求，返回条数。Stop 走这里。
    ///
    /// 不需要 orchestrator 锁，理由和 `CancelRegistry` 一样：Stop 不能排在它要取消的
    /// 工作后面。
    pub fn refuse_all(&self) -> usize {
        let senders = std::mem::take(&mut *self.pending());
        let mut refused = 0;
        for (_, sender) in senders {
            if sender.send(Decision::Cancelled).is_ok() {
                refused += 1;
            }
        }
        refused
    }
}

/// 让"登记了就一定会被清掉"成为结构保证。
///
/// 结束方式不止 resolve / refuse_all / 超时三条：`ask` 的 future 被丢弃（任务被弃、
/// 运行时关停）时不会走到任何一个显式清理点，sender 就留在登记表里。而登记表是 app 级
/// 的、不按运行重建，于是它只增不减，直到下一次 Stop 把它清空。
struct Registered<'a> {
    registry: &'a ApprovalRegistry,
    id: &'a str,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        self.registry.forget(self.id);
    }
}

/// 工具层手里的批准通道：登记表 + 事件出口 + 超时。
///
/// 事件走 `RunEvents` 而不是 `AppHandle`，理由和其他既做决定又发事件的东西一样：
/// 这里的判断（超时算拒绝、结束一定要关对话框）只有在测试能读到发出的事件时才验证得了。
#[derive(Clone)]
pub struct ApprovalGate {
    registry: ApprovalRegistry,
    events: Arc<dyn RunEvents>,
    timeout: Duration,
}

impl std::fmt::Debug for ApprovalGate {
    /// `WorkspaceToolPermissions` derive 了 `Debug`，而 `Arc<dyn RunEvents>` 没有。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalGate")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl ApprovalGate {
    pub fn new(registry: ApprovalRegistry, events: Arc<dyn RunEvents>) -> Self {
        Self {
            registry,
            events,
            timeout: DEFAULT_APPROVAL_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 问一次，等一个决定。
    ///
    /// `cancel` 是这次运行的副作用开关，登记**之后**必须再看一次：Stop 是"拉开关 + 拒掉
    /// 所有挂起请求"两步，一条恰好在两步之间登记上的请求没有人会拒 —— 界面已经回到空闲，
    /// 而这次工具调用还挂着，一直挂到两分钟超时。调用方在问之前查一次挡不住这个缝，
    /// 因为缝就在"查完"和"登记上"之间。
    pub async fn ask(
        &self,
        request: &ApprovalRequest,
        cancel: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> ApprovalOutcome {
        let receiver = self.registry.register(&request.id);
        // 登记了就一定会被清掉：超时、被拒、乃至整个 future 被丢弃，都走 Drop
        let _registered = Registered {
            registry: &self.registry,
            id: &request.id,
        };
        if is_cancelled(cancel) {
            // 还没发过请求，所以也不用发关闭事件 —— 没有对话框被打开过
            return ApprovalOutcome::Cancelled;
        }
        self.events
            .emit_json(APPROVAL_REQUESTED_EVENT, request.payload());

        let outcome = match tokio::time::timeout(self.timeout, receiver).await {
            Ok(Ok(Decision::Approved)) => ApprovalOutcome::Approved,
            Ok(Ok(Decision::Denied)) => ApprovalOutcome::Denied,
            Ok(Ok(Decision::Cancelled)) => ApprovalOutcome::Cancelled,
            // 一个答案送到了批准的等待方：只有前端把选择题的 id 发给了 `answer_agent_question`
            // 之外的入口才可能发生。当成拒绝而不是当成批准 —— 没人按过那个按钮。
            Ok(Ok(Decision::Answered(_))) => ApprovalOutcome::Denied,
            // 发送端被丢弃而没有发送：登记表被清掉了。归到 `Cancelled` 而不是 `Denied` ——
            // 那同样不是某个人对这次动作说的不。
            Ok(Err(_)) => ApprovalOutcome::Cancelled,
            Err(_) => ApprovalOutcome::TimedOut,
        };

        self.events.emit_json(
            APPROVAL_CLOSED_EVENT,
            serde_json::json!({ "id": request.id }),
        );
        outcome
    }

    /// 问一道选择题，等一个答案。
    ///
    /// 结构和 `ask` 完全一样（登记 → 再查一次 Stop → 发事件 → 等 → 关对话框），因为要守的
    /// 不变量一样：没人应答不能当成默认答案，Stop 之后不能再拿到答案，停止等待一定要让
    /// 对话框消失。区别只在回来的是一个字符串而不是一个是/否。
    pub async fn ask_question(
        &self,
        request: &QuestionRequest,
        cancel: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> QuestionOutcome {
        let receiver = self.registry.register(&request.id);
        let _registered = Registered {
            registry: &self.registry,
            id: &request.id,
        };
        if is_cancelled(cancel) {
            return QuestionOutcome::Unanswered(ApprovalOutcome::Cancelled);
        }
        self.events
            .emit_json(QUESTION_REQUESTED_EVENT, request.payload());

        let outcome = match tokio::time::timeout(self.timeout, receiver).await {
            Ok(Ok(Decision::Answered(answer))) => QuestionOutcome::Answered(answer),
            // 用户把提问框关掉了。前端送的是"拒绝"，因为那个对话框上没有"批准"可按
            Ok(Ok(Decision::Denied)) => QuestionOutcome::Unanswered(ApprovalOutcome::Denied),
            Ok(Ok(Decision::Cancelled)) => QuestionOutcome::Unanswered(ApprovalOutcome::Cancelled),
            // 同上一个方向的对称情况：一次批准点到了选择题的 id。没有答案就是没有答案，
            // 绝不能编一个 —— 编出来的那个会被模型当成用户的偏好带到后面每一步。
            Ok(Ok(Decision::Approved)) => QuestionOutcome::Unanswered(ApprovalOutcome::Denied),
            Ok(Err(_)) => QuestionOutcome::Unanswered(ApprovalOutcome::Cancelled),
            Err(_) => QuestionOutcome::Unanswered(ApprovalOutcome::TimedOut),
        };

        self.events.emit_json(
            APPROVAL_CLOSED_EVENT,
            serde_json::json!({ "id": request.id }),
        );
        outcome
    }
}

fn is_cancelled(cancel: Option<&Arc<std::sync::atomic::AtomicBool>>) -> bool {
    cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::SeqCst))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::events::RecordingEvents;

    fn gate(events: &Arc<RecordingEvents>) -> (ApprovalRegistry, ApprovalGate) {
        let registry = ApprovalRegistry::new();
        let gate = ApprovalGate::new(registry.clone(), events.clone())
            .with_timeout(Duration::from_millis(200));
        (registry, gate)
    }

    #[tokio::test]
    async fn an_approval_reaches_the_waiting_call() {
        let events = Arc::new(RecordingEvents::new());
        let (registry, gate) = gate(&events);
        let request = ApprovalRequest::new("browser_open", "Open a page", "example.com", "detail");
        let id = request.id.clone();
        let registry_probe = registry.clone();

        let resolver = tokio::spawn(async move {
            // 等到请求真的登记上再回答：直接 resolve 可能在 `ask` 登记之前跑完
            for _ in 0..50 {
                if registry.resolve(&id, true) {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            false
        });

        let outcome = gate.ask(&request, None).await;
        assert!(resolver.await.unwrap(), "resolve 应该找到等待方");
        assert_eq!(outcome, ApprovalOutcome::Approved);
        // 批准也要清登记表，否则同一个 id 会留在里面
        assert_eq!(registry_probe.refuse_all(), 0);
    }

    #[tokio::test]
    async fn an_answer_reaches_the_waiting_question() {
        let events = Arc::new(RecordingEvents::new());
        let (registry, gate) = gate(&events);
        let request = QuestionRequest::new(
            "Which store should the cache use?",
            vec!["Redis".to_string(), "In-memory".to_string()],
        );
        let id = request.id.clone();

        let resolver = tokio::spawn(async move {
            for _ in 0..50 {
                if registry.answer(&id, "In-memory") {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            false
        });

        let outcome = gate.ask_question(&request, None).await;

        assert!(resolver.await.unwrap(), "answer 应该找到等待方");
        assert_eq!(outcome, QuestionOutcome::Answered("In-memory".to_string()));
        // 问题的载荷必须带着选项：没有选项的提问框只能让用户自己写，而模型明明给了候选
        let asked = events.payloads_for(QUESTION_REQUESTED_EVENT);
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0]["options"][0], "Redis");
        // 停止等待一定要关掉对话框，否则用户会对一个没人在等的问题作答
        assert_eq!(events.payloads_for(APPROVAL_CLOSED_EVENT).len(), 1);
    }

    /// 没人回答绝不能变成一个默认答案。
    ///
    /// 编出来的答案会被模型当成用户的偏好带到后面每一步，而用户从没说过那句话 ——
    /// 这比"没拿到答案"糟得多。
    #[tokio::test]
    async fn an_unanswered_question_never_invents_an_answer() {
        let events = Arc::new(RecordingEvents::new());
        let (_registry, gate) = gate(&events);
        let request = QuestionRequest::new("Pick one", vec!["A".to_string(), "B".to_string()]);

        let outcome = gate.ask_question(&request, None).await;

        assert_eq!(
            outcome,
            QuestionOutcome::Unanswered(ApprovalOutcome::TimedOut)
        );
        assert_eq!(events.payloads_for(APPROVAL_CLOSED_EVENT).len(), 1);
    }

    /// Stop 之后挂着的提问要被收掉，和批准一样。
    #[tokio::test]
    async fn stop_refuses_a_pending_question() {
        let events = Arc::new(RecordingEvents::new());
        let (registry, gate) = gate(&events);
        let request = QuestionRequest::new("Pick one", vec!["A".to_string(), "B".to_string()]);
        let probe = registry.clone();

        let stopper = tokio::spawn(async move {
            for _ in 0..50 {
                if probe.refuse_all() > 0 {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            false
        });

        let outcome = gate.ask_question(&request, None).await;

        assert!(stopper.await.unwrap(), "refuse_all 应该找到等待方");
        assert_eq!(
            outcome,
            QuestionOutcome::Unanswered(ApprovalOutcome::Cancelled)
        );
    }

    /// 一次"批准"点到选择题的 id 上不算答案；一个答案送到批准的等待方也不算批准。
    ///
    /// 两条都来自同一张登记表和同一个 id 空间：陈旧的前端、同时开着的两种对话框都能把
    /// 决定送错地方，而这两种错配里"编一个答案"和"授权一次动作"都是不能接受的结果。
    #[tokio::test]
    async fn a_decision_sent_to_the_wrong_kind_of_wait_is_never_a_yes() {
        let events = Arc::new(RecordingEvents::new());
        let (registry, gate) = gate(&events);

        let question = QuestionRequest::new("Pick one", vec!["A".to_string(), "B".to_string()]);
        let question_id = question.id.clone();
        let registry_for_question = registry.clone();
        let approver = tokio::spawn(async move {
            for _ in 0..50 {
                if registry_for_question.resolve(&question_id, true) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
        let question_outcome = gate.ask_question(&question, None).await;
        approver.await.unwrap();
        assert_eq!(
            question_outcome,
            QuestionOutcome::Unanswered(ApprovalOutcome::Denied)
        );

        let approval = ApprovalRequest::new("browser_open", "Open a page", "example.com", "detail");
        let approval_id = approval.id.clone();
        let answerer = tokio::spawn(async move {
            for _ in 0..50 {
                if registry.answer(&approval_id, "Redis") {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
        let approval_outcome = gate.ask(&approval, None).await;
        answerer.await.unwrap();
        assert_eq!(approval_outcome, ApprovalOutcome::Denied);
    }

    #[tokio::test]
    async fn nobody_answering_is_a_refusal_and_the_dialog_is_closed() {
        let events = Arc::new(RecordingEvents::new());
        let (registry, gate) = gate(&events);
        let request = ApprovalRequest::new("browser_open", "Open a page", "example.com", "detail");

        let outcome = gate.ask(&request, None).await;

        assert_eq!(outcome, ApprovalOutcome::TimedOut);
        assert!(outcome.refusal_detail().is_some());
        // 关闭事件是这条不变量里最容易漏的一半：只发请求不发关闭，超时之后对话框
        // 还开着，用户点"批准"却没有任何东西在等他。
        let closed = events.payloads_for(APPROVAL_CLOSED_EVENT);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0]["id"], serde_json::json!(request.id));
        // 超时之后登记表里不该还留着这条
        assert_eq!(registry.refuse_all(), 0);
    }

    /// Stop 落在"拉开关"和"拒挂起请求"之间时，这条请求也必须立刻被拒。
    ///
    /// 这是调用方在问人之前查一次开关**挡不住**的那个缝：缝就在"查完"和"登记上"之间。
    /// 少了这一条，界面已经回到空闲，而这次工具调用要挂满两分钟才被拒，而且不会有
    /// 任何对话框被关掉的痕迹。
    #[tokio::test]
    async fn a_run_cancelled_before_the_prompt_never_asks() {
        let events = Arc::new(RecordingEvents::new());
        let (registry, gate) = gate(&events);
        let request = ApprovalRequest::new("browser_open", "Open a page", "example.com", "detail");
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(true));

        let outcome = gate.ask(&request, Some(&cancel)).await;

        assert_eq!(outcome, ApprovalOutcome::Cancelled);
        // 没问就不该有框：弹一个没人在等的框比不弹更糟
        assert_eq!(events.count(APPROVAL_REQUESTED_EVENT), 0);
        assert_eq!(events.count(APPROVAL_CLOSED_EVENT), 0);
        assert_eq!(registry.refuse_all(), 0);
    }

    /// 等待方被整体丢弃（任务被弃、运行时关停）时，登记表也要干净。
    ///
    /// 这是第四条结束路径，`resolve` / `refuse_all` / 超时都碰不到它；登记表是 app 级的、
    /// 不按运行重建，漏了就只增不减。
    #[tokio::test]
    async fn a_dropped_waiter_does_not_leak_its_slot() {
        let events = Arc::new(RecordingEvents::new());
        let (registry, gate) = gate(&events);
        let request = ApprovalRequest::new("browser_open", "Open a page", "example.com", "detail");

        {
            let pending = gate.ask(&request, None);
            // 推一次让它登记上，然后连 future 一起丢掉
            tokio::select! {
                _ = pending => unreachable!("200ms 的超时不该在这里到期"),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }

        assert_eq!(registry.refuse_all(), 0, "丢弃的等待方不该留下条目");
    }

    #[tokio::test]
    async fn stop_refuses_everything_pending() {
        let events = Arc::new(RecordingEvents::new());
        let (registry, gate) = gate(&events);
        let first = ApprovalRequest::new("browser_open", "Open a page", "a", "a");
        let second = ApprovalRequest::new("browser_open", "Open a page", "b", "b");

        let stopper = registry.clone();
        let stop = tokio::spawn(async move {
            for _ in 0..50 {
                let refused = stopper.refuse_all();
                if refused == 2 {
                    return refused;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            0
        });

        let (one, two) = tokio::join!(gate.ask(&first, None), gate.ask(&second, None));

        assert_eq!(stop.await.unwrap(), 2, "两条挂起的请求都该被 Stop 拒掉");
        // `Cancelled` 而不是 `Denied`：这条同时钉住"Stop 不是超时"和"Stop 不是人的拒绝"。
        // 断言 `Denied` 的话，把 `refuse_all` 写成发 `Denied` 也照样绿，而记录里就会说
        // 用户拒绝了一次他其实只是停掉了的动作。
        assert_eq!(one, ApprovalOutcome::Cancelled);
        assert_eq!(two, ApprovalOutcome::Cancelled);
        assert_eq!(one.record_suffix(), "_cancelled");
        assert_ne!(
            one.refusal_detail(),
            ApprovalOutcome::Denied.refusal_detail()
        );
    }

    #[tokio::test]
    async fn a_denial_is_not_a_timeout() {
        let events = Arc::new(RecordingEvents::new());
        let (registry, gate) = gate(&events);
        let request = ApprovalRequest::new("browser_open", "Open a page", "example.com", "detail");
        let id = request.id.clone();

        let resolver = tokio::spawn(async move {
            for _ in 0..50 {
                if registry.resolve(&id, false) {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            false
        });

        let outcome = gate.ask(&request, None).await;
        assert!(resolver.await.unwrap());
        assert_eq!(outcome, ApprovalOutcome::Denied);
        // 两种拒绝要给出不同的说法，记录才复盘得出来是谁拒的
        assert_ne!(
            outcome.refusal_detail(),
            ApprovalOutcome::TimedOut.refusal_detail()
        );
    }

    #[test]
    fn a_late_click_does_not_pretend_to_have_been_heard() {
        let registry = ApprovalRegistry::new();
        // 没有等待方（超时之后前端才点）：resolve 必须说 false，调用方才不会把
        // "已经放弃的动作"当成刚刚被批准。
        assert!(!registry.resolve("unknown-id", true));
    }
}
