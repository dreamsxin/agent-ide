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
  agentStateMessageKey,
  isAgentBusy,
  summarizeAgentRun,
} from "../../utils/agentExperience";
import { useT } from "../../i18n";
import type { MessageKey } from "../../i18n/messages";
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
  const t = useT();
  const state = useAgentStore((store) => store.state);
  const steps = useAgentStore((store) => store.steps);
  const diffs = useAgentStore((store) => store.diffs);
  const currentTask = useAgentStore((store) => store.currentTask);
  const ideMode = useAgentStore((store) => store.ideMode);
  const mode = useAgentStore((store) => store.mode);
  const summary = summarizeAgentRun(steps, diffs);
  const StatusIcon = statusIcon(state);
  const detail = runDetail(
    state,
    summary,
    t(`topbar.ideMode.${ideMode}`),
    t(`mode.${mode}`)
  );

  return (
    <section
      data-testid="agent-run-summary"
      className="flex-shrink-0 border-b border-surface-border bg-surface-base/35 px-3 py-2"
      aria-label={t("summary.aria")}
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
              {currentTask?.title || summary.activeStep?.title || t("summary.currentTask")}
            </span>
            <span className={`flex-shrink-0 text-[10px] ${statusStyle[state]}`}>
              {t(agentStateMessageKey(state))}
            </span>
          </div>
          <div className="mt-0.5 truncate text-[10px] text-surface-muted">
            {t(detail.key, detail.params)}
          </div>
        </div>
        <button
          type="button"
          onClick={onOpenPlan}
          aria-label={t("summary.plan.aria", {
            done: summary.completedSteps,
            total: summary.totalSteps,
          })}
          className="inline-flex h-7 items-center gap-1 rounded border border-surface-border px-1.5 text-[10px] text-surface-muted transition-colors hover:bg-surface-border/30 hover:text-surface-text"
          title={t("summary.plan.title")}
        >
          <ListChecks aria-hidden="true" className="h-3.5 w-3.5" />
          <span>{summary.completedSteps}/{summary.totalSteps}</span>
        </button>
        <button
          type="button"
          onClick={onOpenChanges}
          aria-label={
            summary.reviewRequired
              ? t("summary.changes.aria.review", { count: summary.pendingChanges })
              : t("summary.changes.aria")
          }
          className={`inline-flex h-7 items-center gap-1 rounded border px-1.5 text-[10px] transition-colors ${
            summary.pendingChanges > 0
              ? "border-diff-modify/50 bg-diff-modify/10 text-diff-modify hover:bg-diff-modify/20"
              : "border-surface-border text-surface-muted hover:bg-surface-border/30 hover:text-surface-text"
          }`}
          title={t("summary.changes.title")}
        >
          <FileDiff aria-hidden="true" className="h-3.5 w-3.5" />
          <span>
            {summary.reviewRequired
              ? t("summary.changes.count", { count: summary.pendingChanges })
              : "0"}
          </span>
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
  ideModeLabel: string,
  modeLabel: string
): { key: MessageKey; params?: Record<string, string | number> } {
  if (summary.reviewRequired) {
    return summary.pendingChanges === 1
      ? { key: "summary.detail.review.one" }
      : { key: "summary.detail.review.many", params: { count: summary.pendingChanges } };
  }
  if (state === "waiting_user") return { key: "summary.detail.waiting" };
  if (summary.activeStep) {
    return { key: "summary.detail.now", params: { title: summary.activeStep.title } };
  }
  if (summary.nextStep) {
    return { key: "summary.detail.next", params: { title: summary.nextStep.title } };
  }
  // 两个模式名都借顶栏那两份文案：这里曾经直接渲染枚举值，于是同一个模式在顶栏是
  // 「先做计划」，在这行是 `plan`。
  return { key: "summary.detail.mode", params: { ide: ideModeLabel, mode: modeLabel } };
}

function statusIcon(state: AgentState) {
  if (isAgentBusy(state)) return LoaderCircle;
  if (state === "done") return CheckCircle2;
  if (state === "error") return AlertCircle;
  if (state === "waiting_user" || state === "reviewing") return CircleDot;
  return CircleDot;
}
