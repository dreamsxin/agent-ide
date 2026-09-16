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
/// 后端已经不再等这条请求了（超时 / Stop / 已决定），前端该收掉对话框。
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

/// 等待的结果。四种，因为四种要写进记录的话不一样。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalOutcome {
    Approved,
    /// 人点了拒绝，或者 Stop 把挂起的请求全部拒掉
    Denied,
    /// 没人在规定时间内应答
    TimedOut,
    /// 这次运行没有批准通道（headless 入口）。仍然是拒绝，只是原因不同 ——
    /// 记录里要能区分"用户不同意"和"根本没人可问"。
    Unattended,
}

impl ApprovalOutcome {
    pub fn approved(&self) -> bool {
        matches!(self, ApprovalOutcome::Approved)
    }

    /// 写进外部动作记录和工具返回值的那句话。
    pub fn refusal_detail(&self) -> &'static str {
        match self {
            ApprovalOutcome::Approved => "Approved by the user.",
            ApprovalOutcome::Denied => "The user denied this action.",
            ApprovalOutcome::TimedOut => {
                "Nobody approved this action in time, so it was not performed."
            }
            ApprovalOutcome::Unattended => {
                "This run has no approval prompt attached, so the action was refused."
            }
        }
    }
}

enum Decision {
    Approved,
    Denied,
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

    fn register(&self, id: &str) -> oneshot::Receiver<Decision> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id.to_string(), tx);
        }
        rx
    }

    fn forget(&self, id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(id);
        }
    }

    /// 送回一个决定。返回是否真的有人在等这条请求 —— 前端可能在超时之后才点。
    pub fn resolve(&self, id: &str, approved: bool) -> bool {
        let sender = match self.pending.lock() {
            Ok(mut pending) => pending.remove(id),
            Err(_) => None,
        };
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

    /// 拒掉所有挂起的请求，返回条数。Stop 走这里。
    ///
    /// 不需要 orchestrator 锁，理由和 `CancelRegistry` 一样：Stop 不能排在它要取消的
    /// 工作后面。
    pub fn refuse_all(&self) -> usize {
        let senders = match self.pending.lock() {
            Ok(mut pending) => std::mem::take(&mut *pending),
            Err(_) => HashMap::new(),
        };
        let mut refused = 0;
        for (_, sender) in senders {
            if sender.send(Decision::Denied).is_ok() {
                refused += 1;
            }
        }
        refused
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
    pub async fn ask(&self, request: &ApprovalRequest) -> ApprovalOutcome {
        let receiver = self.registry.register(&request.id);
        self.events
            .emit_json(APPROVAL_REQUESTED_EVENT, request.payload());

        let outcome = match tokio::time::timeout(self.timeout, receiver).await {
            Ok(Ok(Decision::Approved)) => ApprovalOutcome::Approved,
            Ok(Ok(Decision::Denied)) => ApprovalOutcome::Denied,
            // 发送端被丢弃而没有发送：登记表被清掉了。当拒绝处理，不当"继续"。
            Ok(Err(_)) => ApprovalOutcome::Denied,
            Err(_) => {
                self.registry.forget(&request.id);
                ApprovalOutcome::TimedOut
            }
        };

        self.events.emit_json(
            APPROVAL_CLOSED_EVENT,
            serde_json::json!({ "id": request.id }),
        );
        outcome
    }
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

        let outcome = gate.ask(&request).await;
        assert!(resolver.await.unwrap(), "resolve 应该找到等待方");
        assert_eq!(outcome, ApprovalOutcome::Approved);
    }

    #[tokio::test]
    async fn nobody_answering_is_a_refusal_and_the_dialog_is_closed() {
        let events = Arc::new(RecordingEvents::new());
        let (_registry, gate) = gate(&events);
        let request = ApprovalRequest::new("browser_open", "Open a page", "example.com", "detail");

        let outcome = gate.ask(&request).await;

        assert_eq!(outcome, ApprovalOutcome::TimedOut);
        assert!(!outcome.approved());
        // 关闭事件是这条不变量里最容易漏的一半：只发请求不发关闭，超时之后对话框
        // 还开着，用户点"批准"却没有任何东西在等他。
        let closed = events.payloads_for(APPROVAL_CLOSED_EVENT);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0]["id"], serde_json::json!(request.id));
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

        let (one, two) = tokio::join!(gate.ask(&first), gate.ask(&second));

        assert_eq!(stop.await.unwrap(), 2, "两条挂起的请求都该被 Stop 拒掉");
        assert_eq!(one, ApprovalOutcome::Denied);
        assert_eq!(two, ApprovalOutcome::Denied);
        // 拒绝要在超时之前到：否则这个测试测的是超时，不是 Stop
        assert_eq!(registry.refuse_all(), 0);
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

        let outcome = gate.ask(&request).await;
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
