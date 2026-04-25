import { useAgentStore } from "../../stores/useAgentStore";
import type { PipelineStage } from "../../types/agent";

interface TaskPipelineProps {
  stages?: PipelineStage[];
}

export default function TaskPipeline({ stages }: TaskPipelineProps) {
  const storePipeline = useAgentStore((s) => s.pipeline);
  const displayStages = stages ?? storePipeline;

  return (
    <div className="p-3 text-xs">
      <div className="text-surface-muted mb-3 font-semibold tracking-wide">
        Pipeline
      </div>

      <div className="flex flex-col gap-0">
        {displayStages.map((stage, i) => {
          const isLast = i === displayStages.length - 1;

          const statusColor: Record<string, string> = {
            pending: "bg-surface-border",
            active: "bg-accent-blue animate-pulse",
            completed: "bg-accent-green",
            failed: "bg-diff-remove",
          };

          const statusText: Record<string, string> = {
            pending: "○",
            active: "◉",
            completed: "●",
            failed: "✕",
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
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
