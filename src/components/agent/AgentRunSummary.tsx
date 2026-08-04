import {
  AlertCircle,
  CheckCircle2,
  CircleDot,
  FileDiff,
  ListChecks,
  LoaderCircle,
} from "lucide-react";
import { useAgentStore } from "../../stores/useAgentStore";
import {
  agentStateLabel,
  isAgentBusy,
  summarizeAgentRun,
} from "../../utils/agentExperience";
import type { AgentState } from "../../types/agent";

interface AgentRunSummaryProps {
  onOpenChanges: () => void;
  onOpenPlan: () => void;
}

const statusStyle: Record<AgentState, string> = {
  idle: "text-surface-muted",
  thinking: "text-accent-blue",
  planning: "text-accent-blue",
  acting: "text-accent-blue",
  reviewing: "text-diff-modify",
  waiting_user: "text-diff-modify",
  done: "text-diff-add",
  error: "text-diff-remove",
};

export default function AgentRunSummary({ onOpenChanges, onOpenPlan }: AgentRunSummaryProps) {
  const state = useAgentStore((store) => store.state);
  const steps = useAgentStore((store) => store.steps);
  const diffs = useAgentStore((store) => store.diffs);
  const currentTask = useAgentStore((store) => store.currentTask);
  const ideMode = useAgentStore((store) => store.ideMode);
  const mode = useAgentStore((store) => store.mode);
  const summary = summarizeAgentRun(steps, diffs);
  const StatusIcon = statusIcon(state);
  const detail = runDetail(state, summary, ideMode, mode);

  return (
    <section
      data-testid="agent-run-summary"
      className="flex-shrink-0 border-b border-surface-border bg-surface-base/35 px-3 py-2"
      aria-label="Current task status"
    >
      <div className="flex min-w-0 items-center gap-2">
        <StatusIcon
          aria-hidden="true"
          className={`h-3.5 w-3.5 flex-shrink-0 ${statusStyle[state]} ${
            isAgentBusy(state) ? "animate-spin" : ""
          }`}
        />
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-baseline gap-2">
            <span className="truncate text-xs font-medium text-surface-text">
              {currentTask?.title || summary.activeStep?.title || "Current task"}
            </span>
            <span className={`flex-shrink-0 text-[10px] ${statusStyle[state]}`}>
              {agentStateLabel(state)}
            </span>
          </div>
          <div className="mt-0.5 truncate text-[10px] text-surface-muted">
            {detail}
          </div>
        </div>
        <button
          type="button"
          onClick={onOpenPlan}
          aria-label={`Open task plan, ${summary.completedSteps} of ${summary.totalSteps} steps complete`}
          className="inline-flex h-7 items-center gap-1 rounded border border-surface-border px-1.5 text-[10px] text-surface-muted transition-colors hover:bg-surface-border/30 hover:text-surface-text"
          title="Open task plan"
        >
          <ListChecks aria-hidden="true" className="h-3.5 w-3.5" />
          <span>{summary.completedSteps}/{summary.totalSteps}</span>
        </button>
        <button
          type="button"
          onClick={onOpenChanges}
          aria-label={
            summary.reviewRequired
              ? `Review ${summary.pendingChanges} proposed changes`
              : "Open proposed changes"
          }
          className={`inline-flex h-7 items-center gap-1 rounded border px-1.5 text-[10px] transition-colors ${
            summary.pendingChanges > 0
              ? "border-diff-modify/50 bg-diff-modify/10 text-diff-modify hover:bg-diff-modify/20"
              : "border-surface-border text-surface-muted hover:bg-surface-border/30 hover:text-surface-text"
          }`}
          title="Review proposed changes"
        >
          <FileDiff aria-hidden="true" className="h-3.5 w-3.5" />
          <span>{summary.reviewRequired ? `Review ${summary.pendingChanges}` : "0"}</span>
        </button>
      </div>
      {summary.totalSteps > 0 && (
        <div className="mt-2 h-1 overflow-hidden rounded bg-surface-border/60" aria-hidden="true">
          <div
            className="h-full rounded bg-accent-blue transition-[width] duration-300"
            style={{ width: `${summary.progressPercent}%` }}
          />
        </div>
      )}
    </section>
  );
}

function runDetail(
  state: AgentState,
  summary: ReturnType<typeof summarizeAgentRun>,
  ideMode: string,
  mode: string
) {
  if (summary.reviewRequired) {
    return `${summary.pendingChanges} change${summary.pendingChanges === 1 ? "" : "s"} ready for review`;
  }
  if (state === "waiting_user") return "Input required in the conversation";
  if (summary.activeStep) return `Now: ${summary.activeStep.title}`;
  if (summary.nextStep) return `Next: ${summary.nextStep.title}`;
  return `${ideMode} mode · ${mode} permissions`;
}

function statusIcon(state: AgentState) {
  if (isAgentBusy(state)) return LoaderCircle;
  if (state === "done") return CheckCircle2;
  if (state === "error") return AlertCircle;
  if (state === "waiting_user" || state === "reviewing") return CircleDot;
  return CircleDot;
}
