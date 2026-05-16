import { useAgentStore } from "../../stores/useAgentStore";
import { useLogStore } from "../../stores/useLogStore";
import type { LogEntry } from "../../types/project";
import type { PipelineStage } from "../../types/agent";

interface TaskPipelineProps {
  stages?: PipelineStage[];
}

export default function TaskPipeline({ stages }: TaskPipelineProps) {
  const storePipeline = useAgentStore((s) => s.pipeline);
  const continueAgentPipeline = useAgentStore((s) => s.continueAgentPipeline);
  const isStreaming = useAgentStore((s) => s.isStreaming);
  const logs = useLogStore((s) => s.logs);
  const displayStages = stages ?? storePipeline;
  const pausedStage = displayStages.find((stage) => stage.status === "paused");

  return (
    <div className="p-3 text-xs">
      <div className="text-surface-muted mb-3 font-semibold tracking-wide flex items-center justify-between gap-2">
        <span>Pipeline</span>
        {pausedStage && (
          <button
            disabled={isStreaming}
            onClick={() => void continueAgentPipeline()}
            className="rounded border border-accent-blue/40 bg-accent-blue/10 px-2 py-0.5 text-[10px] font-normal text-accent-blue hover:bg-accent-blue/20 disabled:cursor-not-allowed disabled:opacity-40"
          >
            Continue
          </button>
        )}
      </div>

      <div className="flex flex-col gap-0">
        {displayStages.map((stage, i) => {
          const isLast = i === displayStages.length - 1;
          const stageLogs = logsForStage(logs, stage);
          const latestLog = stageLogs.length > 0 ? stageLogs[stageLogs.length - 1] : undefined;
          const sourceSummary = latestContextSummary(stageLogs);
          const diffSummary = latestDiffSummary(stageLogs);
          const outputState = stageOutputState(stage, stageLogs);

          const statusColor: Record<string, string> = {
            pending: "bg-surface-border",
            active: "bg-accent-blue animate-pulse",
            completed: "bg-accent-green",
            failed: "bg-diff-remove",
            paused: "bg-diff-modify",
          };

          const statusText: Record<string, string> = {
            pending: "○",
            active: "◉",
            completed: "●",
            failed: "✕",
            paused: "Ⅱ",
          };

          return (
            <div key={`${stage.role}-${i}`} className="flex items-start gap-2">
              {/* Timeline indicator */}
              <div className="flex flex-col items-center flex-shrink-0 pt-0.5">
                <div
                  className={`w-2.5 h-2.5 rounded-full ${statusColor[stage.status]}`}
                />
                {!isLast && (
                  <div
                    className={`w-0.5 h-6 ${
                      stage.status === "completed"
                        ? "bg-accent-green"
                        : stage.status === "active"
                          ? "bg-accent-blue/50"
                          : "bg-surface-border"
                    }`}
                  />
                )}
              </div>

              {/* Stage info */}
              <div className="pb-4 min-w-0 flex-1">
                <div className="flex items-center gap-1.5">
                  <span
                    className={`font-medium ${
                      stage.status === "active"
                        ? "text-accent-blue"
                        : stage.status === "completed"
                          ? "text-accent-green"
                          : stage.status === "failed"
                            ? "text-diff-remove"
                            : stage.status === "paused"
                              ? "text-diff-modify"
                            : "text-surface-muted"
                    }`}
                  >
                    {stage.name}
                  </span>
                  <span className="text-[10px] text-surface-muted">
                    {statusText[stage.status]}
                  </span>
                </div>
                <div className="text-[10px] text-surface-muted capitalize">
                  {stage.role}
                  {stage.pauseBefore ? " · pauses before run" : ""}
                </div>
                <div className="mt-1 flex flex-wrap gap-1">
                  <span className={outputState.className}>{outputState.label}</span>
                  {sourceSummary && (
                    <span
                      title={sourceSummary}
                      className="max-w-full truncate rounded border border-surface-border bg-surface-base px-1.5 py-0.5 text-[10px] text-surface-muted"
                    >
                      input: {sourceSummary}
                    </span>
                  )}
                  {diffSummary && (
                    <span
                      title={diffSummary}
                      className="max-w-full truncate rounded border border-accent-blue/30 bg-accent-blue/10 px-1.5 py-0.5 text-[10px] text-accent-blue"
                    >
                      diff: {diffSummary}
                    </span>
                  )}
                </div>
                {latestLog && (
                  <div className="mt-1 truncate text-[10px] text-surface-muted" title={latestLog.message}>
                    {latestLog.level}: {latestLog.message}
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function logsForStage(logs: LogEntry[], stage: PipelineStage) {
  const stageName = stage.name.toLowerCase();
  const roleName = stage.role.toLowerCase();
  return logs.filter((log) => {
    if (log.source !== "agent") return false;
    const logStage = log.stage?.toLowerCase() ?? "";
    const logRole = log.role?.toLowerCase() ?? "";
    const logPhase = log.phase?.toLowerCase() ?? "";
    return (
      logRole === roleName ||
      logStage === stageName ||
      logStage.includes(roleName) ||
      logPhase.includes(roleName)
    );
  });
}

function latestContextSummary(logs: LogEntry[]) {
  return [...logs].reverse().find((log) => Boolean(log.contextSummary))?.contextSummary ?? null;
}

function latestDiffSummary(logs: LogEntry[]) {
  return [...logs].reverse().find((log) => Boolean(log.diffSummary))?.diffSummary ?? null;
}

function stageOutputState(stage: PipelineStage, logs: LogEntry[]) {
  const base = "rounded border px-1.5 py-0.5 text-[10px]";
  if (stage.status === "failed" || logs.some((log) => log.level === "error")) {
    return {
      label: "failed",
      className: `${base} border-diff-remove/40 bg-diff-remove/10 text-diff-remove`,
    };
  }
  if (stage.status === "paused") {
    return {
      label: "waiting approval",
      className: `${base} border-diff-modify/40 bg-diff-modify/10 text-diff-modify`,
    };
  }
  if (latestDiffSummary(logs)) {
    return {
      label: "produced diff",
      className: `${base} border-accent-blue/40 bg-accent-blue/10 text-accent-blue`,
    };
  }
  if (stage.status === "completed") {
    return {
      label: "no diff",
      className: `${base} border-accent-green/40 bg-accent-green/10 text-accent-green`,
    };
  }
  if (stage.status === "active") {
    return {
      label: "running",
      className: `${base} border-accent-blue/40 bg-accent-blue/10 text-accent-blue`,
    };
  }
  return {
    label: "pending",
    className: `${base} border-surface-border bg-surface-base text-surface-muted`,
  };
}
