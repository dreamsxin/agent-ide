import { PROJECT_TASKS, useTaskStore } from "../../stores/useTaskStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useLogStore } from "../../stores/useLogStore";
import { useProblemStore } from "../../stores/useProblemStore";
import { isTauriRuntime } from "../../utils/tauri";

export default function TasksPanel() {
  const queueTerminalCommand = useTaskStore((s) => s.queueTerminalCommand);
  const lastTask = useTaskStore((s) => s.lastTask);
  const setBottomTab = useLayoutStore((s) => s.setBottomTab);
  const bottomVisible = useLayoutStore((s) => s.bottomVisible);
  const toggleBottomPanel = useLayoutStore((s) => s.toggleBottomPanel);
  const addLog = useLogStore((s) => s.addLog);
  const clearProblems = useProblemStore((s) => s.clearProblems);

  const runTask = (taskId: (typeof PROJECT_TASKS)[number]["id"], command: string, label: string) => {
    if (!bottomVisible) {
      toggleBottomPanel();
    }
    setBottomTab("terminal");
    clearProblems("test");
    queueTerminalCommand(taskId, command);
    addLog({
      time: new Date().toLocaleTimeString(),
      level: "info",
      source: "system",
      message: `Queued project task: ${label}`,
      details: command,
    });
  };

  return (
    <div className="flex h-full flex-col bg-black text-xs">
      <div className="border-b border-surface-border px-3 py-2">
        <div className="font-semibold text-surface-text">Project Tasks</div>
        <div className="mt-0.5 text-[11px] text-surface-muted">
          Run common build, test, lint, run, and debug commands in the integrated terminal.
        </div>
      </div>

      {!isTauriRuntime() && (
        <div className="border-b border-surface-border px-3 py-2 text-[11px] text-diff-modify">
          Project tasks run in the Tauri app runtime.
        </div>
      )}

      <div className="grid grid-cols-1 gap-2 overflow-auto p-3 sm:grid-cols-2 xl:grid-cols-3">
        {PROJECT_TASKS.map((task) => (
          <button
            key={task.id}
            onClick={() => runTask(task.id, task.command, task.label)}
            disabled={!isTauriRuntime()}
            className="rounded border border-surface-border bg-surface-panel p-3 text-left transition-colors hover:border-accent-blue/50 hover:bg-surface-border/20 disabled:cursor-not-allowed disabled:opacity-50"
          >
            <div className="flex items-center justify-between gap-2">
              <span className="font-semibold text-surface-text">{task.label}</span>
              <span className="rounded border border-surface-border px-1.5 py-0.5 font-mono text-[10px] uppercase text-surface-muted">
                {task.id}
              </span>
            </div>
            <div className="mt-2 font-mono text-[11px] text-accent-blue">{task.command}</div>
            <div className="mt-2 text-[11px] leading-relaxed text-surface-muted">
              {task.description}
            </div>
          </button>
        ))}
      </div>

      {lastTask && (
        <div className="border-t border-surface-border px-3 py-2 text-[11px] text-surface-muted">
          Last queued: <span className="font-mono text-surface-text">{lastTask.command}</span>
        </div>
      )}
    </div>
  );
}
