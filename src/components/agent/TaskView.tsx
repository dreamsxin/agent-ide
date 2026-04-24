import type { Step } from "../../types/agent";

const statusConfig: Record<Step["status"], { icon: string; color: string }> = {
  todo: { icon: "○", color: "text-surface-muted" },
  doing: { icon: "◉", color: "text-accent-blue" },
  done: { icon: "●", color: "text-diff-add" },
  error: { icon: "✕", color: "text-diff-remove" },
};

interface TaskViewProps {
  steps?: Step[];
  title?: string;
}

/** 演示用 mock steps */
const mockSteps: Step[] = [
  { id: "1", title: "Create auth module", type: "create", status: "done", logs: [] },
  { id: "2", title: "Add JWT middleware", type: "edit", status: "doing", logs: [] },
  { id: "3", title: "Write unit tests", type: "test", status: "todo", logs: [] },
  { id: "4", title: "Error handling", type: "edit", status: "todo", logs: [] },
];

export default function TaskView({ steps = mockSteps, title = "Task: Authentication" }: TaskViewProps) {
  return (
    <div className="p-3 space-y-2 animate-fade-in">
      {/* 任务标题 */}
      <div className="text-xs font-semibold text-surface-text mb-3">{title}</div>

      {/* 步骤列表 */}
      {steps.map((step) => {
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
            <span className={`${config.color} ${step.status === "doing" ? "animate-pulse-dot" : ""}`}>
              {config.icon}
            </span>
            <span
              className={`flex-1 truncate ${
                step.status === "done" ? "line-through text-surface-muted" : "text-surface-text"
              }`}
            >
              {step.title}
            </span>
            {step.status === "error" && (
              <button className="text-[10px] text-accent-blue hover:underline">Retry</button>
            )}
          </div>
        );
      })}

      {/* 空状态 */}
      {steps.length === 0 && (
        <div className="text-xs text-surface-muted text-center py-6">
          No active task. Start a conversation in Chat.
        </div>
      )}
    </div>
  );
}
