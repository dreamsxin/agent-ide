import { useAgentStore } from "../../stores/useAgentStore";
import { useEditorStore } from "../../stores/useEditorStore";
import type { Step } from "../../types/agent";

const statusConfig: Record<Step["status"], { icon: string; color: string }> = {
  todo: { icon: "○", color: "text-surface-muted" },
  doing: { icon: "◉", color: "text-accent-blue" },
  done: { icon: "●", color: "text-diff-add" },
  error: { icon: "✕", color: "text-diff-remove" },
  skipped: { icon: "⊘", color: "text-surface-muted" },
};

export default function TaskView() {
  const steps = useAgentStore((s) => s.steps);
  const agentState = useAgentStore((s) => s.state);
  const currentTask = useAgentStore((s) => s.currentTask);
  const agentRunId = useAgentStore((s) => s.agentRunId);
  const restoredSession = useAgentStore((s) => s.restoredSession);
  const chatProfileId = useAgentStore((s) => s.chatProfileId);
  const activeProfileId = useAgentStore((s) => s.activeProfileId);
  const chatContextCompression = useAgentStore((s) => s.chatContextCompression);
  const contextCompression = useAgentStore((s) => s.contextCompression);
  const updateAgentStep = useAgentStore((s) => s.updateAgentStep);
  const skipAgentStep = useAgentStore((s) => s.skipAgentStep);
  const runAgentStep = useAgentStore((s) => s.runAgentStep);
  const clearAgentSession = useAgentStore((s) => s.clearAgentSession);
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const fileContents = useEditorStore((s) => s.fileContents);
  const selectedText = useEditorStore((s) => s.selectedText);

  const title = currentTask?.title ?? "Agent Task";
  const canRun = agentState === "idle" || agentState === "done" || agentState === "waiting_user" || agentState === "error";

  const updateStepField = async (step: Step, updates: Partial<Step>) => {
    await updateAgentStep({ ...step, ...updates });
  };

  const runStep = async (step: Step, moreContext = false) => {
    await runAgentStep({
      step,
      activeFile: activeFile ?? undefined,
      activeFileContent: activeFile ? fileContents[activeFile] : undefined,
      selection: selectedText ?? undefined,
      contextFiles: openFiles.map((file) => file.path),
      profileId: chatProfileId ?? activeProfileId,
      contextCompression: chatContextCompression ?? contextCompression,
      contextSources: {
        includeGitDiff: moreContext || true,
        includeProjectTree: moreContext || true,
      },
      extraPrompt: moreContext
        ? "Regenerate this step with broader context. Include workspace tree, git diff, Problems, failed run output, terminal output, and recent warning/error logs when available."
        : undefined,
    });
  };

  return (
    <div className="p-3 space-y-2 animate-fade-in h-full overflow-auto">
      {/* 任务标题 + 状态 */}
      <div className="flex items-center justify-between mb-3">
        <span className="text-xs font-semibold text-surface-text">{title}</span>
        <span className="text-[10px] text-surface-muted capitalize">{agentState}</span>
      </div>
      {steps.length > 0 && restoredSession && (
        <div className="rounded border border-diff-modify/30 bg-diff-modify/10 px-2 py-1.5 text-[11px] text-surface-muted">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0 flex-1">
              <div className="text-surface-text">
                {restoredSession.interrupted
                  ? "Restored interrupted Agent task."
                  : "Restored Agent task state."}
              </div>
              <div className="mt-0.5 truncate font-mono text-[10px]">
                {(agentRunId ?? restoredSession.runId) || "no-run-id"} · {formatRestoreTime(restoredSession.restoredAt)}
              </div>
              <div className="mt-0.5">
                Review diffs or run a step to continue.
              </div>
            </div>
            <button
              onClick={clearAgentSession}
              className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] hover:bg-surface-border/30"
            >
              Clear
            </button>
          </div>
        </div>
      )}

      {/* 步骤列表 */}
      {steps.length > 0 ? (
        steps.map((step) => {
          const config = statusConfig[step.status];
          return (
            <div
              key={step.id}
              className={`space-y-2 px-2 py-2 rounded text-xs border border-transparent transition-colors ${
                step.status === "doing"
                  ? "bg-accent-blue/10 border-accent-blue/30"
                  : "hover:bg-surface-border/20"
              }`}
            >
              <div className="flex items-center gap-2">
                <span
                  className={`${config.color} ${
                    step.status === "doing" ? "animate-pulse-dot" : ""
                  }`}
                >
                  {config.icon}
                </span>
                <input
                  value={step.title}
                  onChange={(event) => void updateStepField(step, { title: event.target.value })}
                  className={`min-w-0 flex-1 rounded border border-transparent bg-transparent px-1 py-0.5 outline-none focus:border-accent-blue focus:bg-surface-base ${
                    step.status === "done" || step.status === "skipped"
                      ? "text-surface-muted"
                      : "text-surface-text"
                  }`}
                />
              </div>
              <div className="grid grid-cols-2 gap-1">
                <select
                  value={step.scope ?? "workspace"}
                  onChange={(event) => void updateStepField(step, { scope: event.target.value as Step["scope"] })}
                  className="rounded border border-surface-border bg-surface-base px-1 py-0.5 text-[10px] text-surface-text"
                >
                  <option value="selection">Selection</option>
                  <option value="active_file">Active file</option>
                  <option value="open_files">Open files</option>
                  <option value="workspace">Workspace</option>
                </select>
                <select
                  value={step.executionMode ?? "diff"}
                  onChange={(event) => void updateStepField(step, { executionMode: event.target.value as Step["executionMode"] })}
                  className="rounded border border-surface-border bg-surface-base px-1 py-0.5 text-[10px] text-surface-text"
                >
                  <option value="analyze">Analyze</option>
                  <option value="diff">Diff</option>
                  <option value="test">Test</option>
                  <option value="fix">Fix</option>
                </select>
              </div>
              <div className="flex flex-wrap gap-1">
                <button
                  disabled={!canRun || step.status === "doing"}
                  onClick={() => void runStep(step)}
                  className="rounded border border-accent-blue/40 px-1.5 py-0.5 text-[10px] text-accent-blue hover:bg-accent-blue/10 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  Run only
                </button>
                <button
                  disabled={!canRun || step.status === "doing"}
                  onClick={() => void runStep(step, true)}
                  className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-text hover:bg-surface-border/30 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  Regenerate
                </button>
                <button
                  disabled={step.status === "doing" || step.status === "skipped"}
                  onClick={() => void skipAgentStep(step.id)}
                  className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-muted hover:bg-surface-border/30 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  Skip
                </button>
              </div>
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

function formatRestoreTime(value: number) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "restored";
  return date.toLocaleTimeString();
}
