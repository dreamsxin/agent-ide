import { useMemo, useState } from "react";
import { useTaskStore } from "../../stores/useTaskStore";
import { isTauriRuntime } from "../../utils/tauri";
import { useProjectTasks } from "../../hooks/useProjectTasks";
import { useRunProjectTask } from "../../hooks/useRunProjectTask";
import { useFixWithAgent } from "../../hooks/useFixWithAgent";

export default function TasksPanel() {
  const lastTask = useTaskStore((s) => s.lastTask);
  const taskRuns = useTaskStore((s) => s.taskRuns);
  const taskRunHistory = useTaskStore((s) => s.taskRunHistory);
  const clearTaskRunHistory = useTaskStore((s) => s.clearTaskRunHistory);
  const { tasks, usingFallback, loading, error } = useProjectTasks();
  const runProjectTask = useRunProjectTask();
  const { fixTaskFailure, isAgentBusy } = useFixWithAgent();
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const selectedRun = useMemo(
    () => taskRunHistory.find((run) => run.runId === selectedRunId) ?? taskRunHistory[0],
    [selectedRunId, taskRunHistory]
  );

  const rerunHistoryEntry = (run: typeof selectedRun) => {
    if (!run) return;
    const task = tasks.find((item) => item.id === run.taskId) ?? {
      id: run.taskId,
      label: run.label,
      command: run.command,
      description: "Run from command history.",
      source: "history",
    };
    void runProjectTask(task);
  };

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

      <div className="grid min-h-[180px] grid-cols-[minmax(260px,0.42fr)_minmax(320px,1fr)] border-t border-surface-border">
        <div className="min-w-0 border-r border-surface-border">
          <div className="flex items-center justify-between gap-2 border-b border-surface-border px-3 py-1.5">
            <span className="font-semibold text-surface-text">Run History</span>
            {taskRunHistory.length > 0 && (
              <button
                onClick={clearTaskRunHistory}
                className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-muted hover:text-surface-text"
              >
                Clear
              </button>
            )}
          </div>
          <div className="max-h-44 overflow-auto">
            {taskRunHistory.length === 0 ? (
              <div className="px-3 py-4 text-center text-[11px] text-surface-muted">
                No command runs yet.
              </div>
            ) : (
              taskRunHistory.map((run) => (
                <button
                  key={run.runId}
                  onClick={() => setSelectedRunId(run.runId)}
                  className={`grid w-full grid-cols-[1fr_auto] gap-2 border-b border-surface-border/40 px-3 py-1.5 text-left ${
                    selectedRun?.runId === run.runId
                      ? "bg-accent-blue/10"
                      : "hover:bg-surface-border/20"
                  }`}
                >
                  <span className="min-w-0">
                    <span className="block truncate font-semibold text-surface-text">
                      {run.label}
                    </span>
                    <span className="block truncate font-mono text-[10px] text-surface-muted">
                      {run.command}
                    </span>
                  </span>
                  <span className="text-right">
                    <span className={statusClass(run.status)}>
                      {run.status}
                    </span>
                    <span className="block text-[10px] text-surface-muted">
                      {formatDuration(run.durationMs)}
                    </span>
                  </span>
                </button>
              ))
            )}
          </div>
        </div>

        <div className="min-w-0">
          <div className="flex items-center gap-2 border-b border-surface-border px-3 py-1.5">
            <span className="min-w-0 flex-1 truncate font-semibold text-surface-text">
              {selectedRun ? selectedRun.label : "Output"}
            </span>
            {selectedRun && (
              <>
                <span className="font-mono text-[10px] text-surface-muted">
                  exit {selectedRun.exitCode ?? "unknown"}
                </span>
                <button
                  onClick={() => rerunHistoryEntry(selectedRun)}
                  disabled={!isTauriRuntime()}
                  className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-muted hover:text-surface-text disabled:cursor-not-allowed disabled:opacity-40"
                >
                  Rerun
                </button>
                {selectedRun.status === "failed" && (
                  <button
                    onClick={() => void fixTaskFailure(selectedRun)}
                    disabled={isAgentBusy}
                    className="rounded border border-accent-blue/40 px-1.5 py-0.5 text-[10px] text-accent-blue hover:bg-accent-blue/10 disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    Fix with Agent
                  </button>
                )}
              </>
            )}
          </div>
          <pre className="max-h-44 overflow-auto whitespace-pre-wrap px-3 py-2 font-mono text-[10px] leading-relaxed text-surface-text">
            {selectedRun?.output?.trim() || "Select a run to inspect command output."}
          </pre>
        </div>
      </div>

      {lastTask && (
        <div className="border-t border-surface-border px-3 py-2 text-[11px] text-surface-muted">
          Last queued: <span className="font-mono text-surface-text">{lastTask.command}</span>
        </div>
      )}
    </div>
  );
}

function statusClass(status: string) {
  if (status === "success") return "text-[10px] uppercase text-diff-add";
  if (status === "failed") return "text-[10px] uppercase text-diff-remove";
  if (status === "running") return "text-[10px] uppercase text-accent-blue";
  return "text-[10px] uppercase text-surface-muted";
}

function formatDuration(durationMs?: number) {
  if (durationMs === undefined) return "-";
  if (durationMs < 1000) return `${durationMs} ms`;
  return `${(durationMs / 1000).toFixed(1)} s`;
}
