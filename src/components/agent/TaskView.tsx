import { useAgentStore } from "../../stores/useAgentStore";
import type { Step } from "../../types/agent";

const statusConfig: Record<Step["status"], { icon: string; color: string }> = {
  todo: { icon: "○", color: "text-surface-muted" },
  doing: { icon: "◉", color: "text-accent-blue" },
  done: { icon: "●", color: "text-diff-add" },
  error: { icon: "✕", color: "text-diff-remove" },
};

export default function TaskView() {
  const steps = useAgentStore((s) => s.steps);
  const agentState = useAgentStore((s) => s.state);
  const currentTask = useAgentStore((s) => s.currentTask);

  const title = currentTask?.title ?? "Agent Task";

  return (
    <div className="p-3 space-y-2 animate-fade-in h-full overflow-auto">
      {/* 任务标题 + 状态 */}
      <div className="flex items-center justify-between mb-3">
        <span className="text-xs font-semibold text-surface-text">{title}</span>
        <span className="text-[10px] text-surface-muted capitalize">{agentState}</span>
      </div>

      {/* 步骤列表 */}
      {steps.length > 0 ? (
        steps.map((step) => {
          const config = statusConfig[step.status];
          return (
            <div
              key={step.id}
              className={`flex items-center gap-2 px-2 py-1.5 rounded text-xs border border-transparent transition-colors ${
                step.status === "doing"
                  ? "bg-accent-blue/10 border-accent-blue/30"
                  : "hover:bg-surface-border/20"
              }`}
            >
              <span
                className={`${config.color} ${
                  step.status === "doing" ? "animate-pulse-dot" : ""
                }`}
              >
                {config.icon}
              </span>
              <span
                className={`flex-1 truncate ${
                  step.status === "done"
                    ? "line-through text-surface-muted"
                    : "text-surface-text"
                }`}
              >
                {step.title}
              </span>
              {step.status === "error" && (
                <button className="text-[10px] text-accent-blue hover:underline">
                  Retry
                </button>
              )}
            </div>
          );
        })
      ) : (
        <div className="text-xs text-surface-muted text-center py-6 space-y-2">
          <div>No active task</div>
          <div className="text-[10px]">Start a conversation in Chat to see the task plan here.</div>
        </div>
      )}

      {/* 步骤日志 */}
      {steps.some((s) => s.logs.length > 0) && (
        <div className="mt-4 border-t border-surface-border pt-3">
          <div className="text-[10px] text-surface-muted mb-2">Step Logs</div>
          {steps
            .filter((s) => s.logs.length > 0)
            .map((step) =>
              step.logs.map((log, i) => (
                <div
                  key={`${step.id}-${i}`}
                  className="text-[10px] text-surface-muted font-mono bg-surface-base rounded px-2 py-1 mb-1 whitespace-pre-wrap break-all"
                >
                  {log}
                </div>
              ))
            )}
        </div>
      )}
    </div>
  );
}
