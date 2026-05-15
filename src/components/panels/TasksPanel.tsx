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
      <div className="flex items-center justify-between gap-3 border-b border-surface-border px-3 py-1.5">
        <div className="min-w-0">
          <div className="font-semibold text-surface-text">Commands</div>
          <div className="truncate text-[11px] text-surface-muted">
            {tasks.length} discovered ·{" "}
            {usingFallback ? "fallback commands" : "workspace configuration"}
          </div>
        </div>
        <div className="truncate text-[11px] text-surface-muted">
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

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(260px,0.38fr)_minmax(360px,1fr)]">
        <div className="min-w-0 border-r border-surface-border">
          <div className="grid grid-cols-[minmax(120px,0.9fr)_minmax(180px,1.3fr)_72px] border-b border-surface-border bg-surface-panel/70 px-3 py-1 text-[10px] uppercase text-surface-muted">
            <span>Command</span>
            <span>Script</span>
            <span className="text-right">Status</span>
          </div>
          <div className="h-full overflow-auto">
            {loading && (
              <div className="px-3 py-4 text-center text-[11px] text-surface-muted">
                Loading workspace commands...
              </div>
            )}
            {tasks.map((task) => {
              const runState = taskRuns[task.id];
              return (
                <button
                  key={task.id}
                  onClick={() => void runProjectTask(task)}
                  disabled={!isTauriRuntime()}
                  className="grid w-full grid-cols-[minmax(120px,0.9fr)_minmax(180px,1.3fr)_72px] items-center gap-2 border-b border-surface-border/40 px-3 py-1.5 text-left hover:bg-surface-border/20 disabled:cursor-not-allowed disabled:opacity-50"
                  title={task.description}
                >
                  <span className="min-w-0 truncate font-semibold text-surface-text">
                    {task.label}
                  </span>
                  <span className="min-w-0 truncate font-mono text-[11px] text-accent-blue">
                    {task.command}
                  </span>
                  <span className="text-right">
                    <span className={statusClass(runState?.status ?? task.source)}>
                      {runState?.status ?? task.source}
                    </span>
                    {runState?.status === "failed" && (
                      <button
                        onClick={(event) => {
                          event.stopPropagation();
                          void fixTaskFailure(runState);
                        }}
                        disabled={isAgentBusy}
                        className="ml-1 rounded border border-accent-blue/40 px-1 py-0.5 text-[10px] text-accent-blue hover:bg-accent-blue/10 disabled:cursor-not-allowed disabled:opacity-40"
                      >
                        Fix
                      </button>
                    )}
                  </span>
                </button>
              );
            })}
          </div>
        </div>

        <div className="grid min-w-0 grid-rows-[minmax(96px,0.38fr)_minmax(120px,1fr)]">
          <div className="min-h-0 border-b border-surface-border">
            <div className="flex items-center justify-between gap-2 border-b border-surface-border px-3 py-1.5">
              <span className="font-semibold text-surface-text">Run History</span>
              {taskRunHistory.length > 0 && (
                <button
                  onClick={clearTaskRunHistory}
                  className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-muted hover:text-surface-text disabled:cursor-not-allowed disabled:opacity-40"
                >
                  Clear
                </button>
              )}
            </div>
            <div className="h-full overflow-auto">
              {taskRunHistory.length === 0 ? (
                <div className="px-3 py-4 text-center text-[11px] text-surface-muted">
                  No command runs yet.
                </div>
              ) : (
                taskRunHistory.map((run) => (
                  <button
                    key={run.runId}
                    onClick={() => setSelectedRunId(run.runId)}
                    className={`grid w-full grid-cols-[1fr_auto_auto] items-center gap-2 border-b border-surface-border/40 px-3 py-1.5 text-left ${
                      selectedRun?.runId === run.runId
                        ? "bg-accent-blue/10"
                        : "hover:bg-surface-border/20"
                    }`}
                  >
                    <span className="min-w-0 truncate font-semibold text-surface-text">
                      {run.label}
                    </span>
                    <span className={statusClass(run.status)}>{run.status}</span>
                    <span className="font-mono text-[10px] text-surface-muted">
                      {formatDuration(run.durationMs)}
                    </span>
                  </button>
                ))
              )}
            </div>
          </div>

          <div className="min-h-0">
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
            <pre className="h-full overflow-auto whitespace-pre-wrap px-3 py-2 font-mono text-[10px] leading-relaxed text-surface-text">
              {selectedRun?.output?.trim() || "Select a run to inspect command output."}
            </pre>
          </div>
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
