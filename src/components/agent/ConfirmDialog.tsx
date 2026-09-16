import { useAgentStore } from "../../stores/useAgentStore";

const OP_LABELS: Record<string, string> = {
  file_delete: "File Deletion",
  command_run: "Command Execution",
  git_push: "Git Push",
  git_force: "Git Force Operation",
  browser_open: "Browser Navigation",
};

const OP_ICONS: Record<string, string> = {
  file_delete: "\u{1F5D1}",
  command_run: "\u{2699}",
  git_push: "\u{1F4E4}",
  git_force: "\u{26A0}",
  browser_open: "\u{1F310}",
};

/**
 * 逐动作批准的提示框。
 *
 * 后端有一次撤不回的动作正**挂在这里等**：`agent-approval-requested` 把它放进
 * `pendingConfirm`，两个按钮各送一个决定回去。所以这个组件不是装饰 —— 不点，动作
 * 就在超时之后被拒掉。
 */
export default function ConfirmDialog() {
  const pendingConfirm = useAgentStore((s) => s.pendingConfirm);
  const resolveConfirm = useAgentStore((s) => s.resolveConfirm);

  if (!pendingConfirm) return null;

  const icon = OP_ICONS[pendingConfirm.opType] ?? "\u{26A0}";
  const label = OP_LABELS[pendingConfirm.opType] ?? pendingConfirm.opType;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="w-full max-w-sm rounded-lg border border-surface-border bg-surface-panel shadow-xl">
        {/* Header */}
        <div className="flex items-center gap-2 border-b border-surface-border px-4 py-3">
          <span className="text-lg">{icon}</span>
          <div>
            <div className="text-sm font-semibold text-surface-text">
              {pendingConfirm.title}
            </div>
            <div className="text-[11px] text-surface-muted">{label}</div>
          </div>
        </div>

        {/* Body */}
        <div className="px-4 py-3">
          <p className="text-xs text-surface-text leading-relaxed">
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
