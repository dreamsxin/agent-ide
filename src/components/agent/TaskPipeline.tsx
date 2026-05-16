import { useAgentStore } from "../../stores/useAgentStore";
import type { PipelineStage } from "../../types/agent";

interface TaskPipelineProps {
  stages?: PipelineStage[];
}

export default function TaskPipeline({ stages }: TaskPipelineProps) {
  const storePipeline = useAgentStore((s) => s.pipeline);
  const continueAgentPipeline = useAgentStore((s) => s.continueAgentPipeline);
  const isStreaming = useAgentStore((s) => s.isStreaming);
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
              <div className="pb-4 min-w-0">
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
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
