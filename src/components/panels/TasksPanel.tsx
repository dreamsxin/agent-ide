import { useTaskStore } from "../../stores/useTaskStore";
import { isTauriRuntime } from "../../utils/tauri";
import { useProjectTasks } from "../../hooks/useProjectTasks";
import { useRunProjectTask } from "../../hooks/useRunProjectTask";
import { useFixWithAgent } from "../../hooks/useFixWithAgent";

export default function TasksPanel() {
  const lastTask = useTaskStore((s) => s.lastTask);
  const taskRuns = useTaskStore((s) => s.taskRuns);
  const { tasks, usingFallback, loading, error } = useProjectTasks();
  const runProjectTask = useRunProjectTask();
  const { fixTaskFailure, isAgentBusy } = useFixWithAgent();

  return (
    <div className="flex h-full flex-col bg-black text-xs">
      <div className="border-b border-surface-border px-3 py-2">
        <div className="font-semibold text-surface-text">Project Commands</div>
        <div className="mt-0.5 text-[11px] text-surface-muted">
          {usingFallback
            ? "No workspace tasks discovered yet. Showing fallback commands."
            : "Tasks discovered from the current workspace configuration."}
        </div>
      </div>

      {!isTauriRuntime() && (
        <div className="border-b border-surface-border px-3 py-2 text-[11px] text-diff-modify">
          Project tasks run in the Tauri app runtime.
        </div>
      )}

      {error && (
        <div className="border-b border-surface-border px-3 py-2 text-[11px] text-diff-remove">
          Failed to discover workspace tasks: {error}
        </div>
      )}

      <div className="grid grid-cols-1 gap-2 overflow-auto p-3 sm:grid-cols-2 xl:grid-cols-3">
        {loading && (
          <div className="rounded border border-surface-border bg-surface-panel p-3 text-surface-muted">
            Loading workspace tasks...
          </div>
        )}
        {tasks.map((task) => {
          const runState = taskRuns[task.id];
          return (
            <div
              key={task.id}
              onClick={() => void runProjectTask(task)}
              className={`rounded border border-surface-border bg-surface-panel p-3 text-left transition-colors hover:border-accent-blue/50 hover:bg-surface-border/20 ${!isTauriRuntime() ? "cursor-not-allowed opacity-50" : "cursor-pointer"}`}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="font-semibold text-surface-text">{task.label}</span>
                <span className="rounded border border-surface-border px-1.5 py-0.5 font-mono text-[10px] uppercase text-surface-muted">
                  {runState?.status ?? task.source}
                </span>
              </div>
              <div className="mt-2 font-mono text-[11px] text-accent-blue">{task.command}</div>
              <div className="mt-2 text-[11px] leading-relaxed text-surface-muted">
                {task.description}
              </div>
              {runState?.status === "failed" && (
                <div className="mt-3 flex items-center gap-2">
                  <button
                    onClick={(event) => {
                      event.stopPropagation();
                      void fixTaskFailure(runState);
                    }}
                    disabled={isAgentBusy}
                    className="rounded border border-accent-blue/40 px-2 py-1 text-[11px] text-accent-blue hover:bg-accent-blue/10 disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    Fix with Agent
                  </button>
                  <span className="text-[10px] text-diff-remove">
                    Exit {runState.exitCode ?? "unknown"}
                  </span>
                </div>
              )}
            </div>
          );
        })}
      </div>

      {lastTask && (
        <div className="border-t border-surface-border px-3 py-2 text-[11px] text-surface-muted">
          Last queued: <span className="font-mono text-surface-text">{lastTask.command}</span>
        </div>
      )}
    </div>
  );
}
