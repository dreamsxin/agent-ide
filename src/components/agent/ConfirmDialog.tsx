import { useEffect } from "react";
import { useAgentStore } from "../../stores/useAgentStore";

const OP_LABELS: Record<string, string> = {
  browser_open: "Browser Navigation",
  browser_read_page: "Page Reading",
  computer_capture: "Window Capture",
  computer_click: "Window Click",
  computer_scroll: "Window Scroll",
};

const OP_ICONS: Record<string, string> = {
  browser_open: "\u{1F310}",
  browser_read_page: "\u{1F4C4}",
  computer_capture: "\u{1F4F8}",
  computer_click: "\u{1F5B1}",
  computer_scroll: "\u{1F503}",
};


/**
 * 逐动作批准的提示框。
 *
 * 后端有一次撤不回的动作正**挂在这里等**：`agent-approval-requested` 把它放进
 * `pendingConfirm`，两个按钮各送一个决定回去。所以这个组件不是装饰 —— 不点，动作
 * 就在超时之后被拒掉。
 *
 * 键盘可达是安全要求而不是打磨：这是一道真的授权关卡，只能用鼠标点的关卡等于对
 * 键盘用户不存在。Esc 走**拒绝**，因为"随手关掉"绝不能等于同意。
 */
export default function ConfirmDialog() {
  const pendingConfirm = useAgentStore((s) => s.pendingConfirm);
  const resolveConfirm = useAgentStore((s) => s.resolveConfirm);

  useEffect(() => {
    if (!pendingConfirm) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void resolveConfirm(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [pendingConfirm, resolveConfirm]);

  if (!pendingConfirm) {
    return null;
  }

  const icon = OP_ICONS[pendingConfirm.opType] ?? "\u{26A0}";
  const label = OP_LABELS[pendingConfirm.opType] ?? pendingConfirm.opType;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="agent-approval-title"
        aria-describedby="agent-approval-description"
        className="w-full max-w-sm rounded-lg border border-surface-border bg-surface-panel shadow-xl"
      >
        {/* Header */}
        <div className="flex items-center gap-2 border-b border-surface-border px-4 py-3">
          <span className="text-lg" aria-hidden="true">
            {icon}
          </span>
          <div>
            <div id="agent-approval-title" className="text-sm font-semibold text-surface-text">
              {pendingConfirm.title}
            </div>
            <div className="text-[11px] text-surface-muted">{label}</div>
          </div>
        </div>

        {/* Body */}
        <div className="px-4 py-3">
          <p id="agent-approval-description" className="text-xs text-surface-text leading-relaxed">
            {pendingConfirm.description}
          </p>
          {pendingConfirm.detail && (
            <p className="mt-2 text-[11px] text-surface-muted font-mono rounded bg-surface-base p-2">
              {pendingConfirm.detail}
            </p>
          )}
        </div>

        {/* Actions */}
        <div className="flex justify-end gap-2 border-t border-surface-border px-4 py-3">
          <button
            // 焦点默认落在 Deny：连按回车不该变成一次授权
            autoFocus
            onClick={() => void resolveConfirm(false)}
            className="rounded border border-surface-border px-4 py-1.5 text-xs text-surface-muted hover:bg-surface-border/30 transition-colors"
          >
            Deny
          </button>
          <button
            onClick={() => void resolveConfirm(true)}
            className="rounded bg-accent-blue px-4 py-1.5 text-xs text-white font-medium hover:bg-accent-blue/80 transition-colors"
          >
            Approve
          </button>
        </div>
      </div>
    </div>
  );
}
