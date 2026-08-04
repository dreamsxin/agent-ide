import type { AgentState, DiffEntry, Step } from "../types/agent";

const REVIEWABLE_DIFF_STATUSES = new Set<DiffEntry["status"]>([
  "pending",
  "partial",
  "failed",
]);

export interface AgentRunSummary {
  activeStep: Step | null;
  completedSteps: number;
  nextStep: Step | null;
  pendingChanges: number;
  progressPercent: number;
  reviewRequired: boolean;
  totalSteps: number;
}

export function summarizeAgentRun(steps: Step[], diffs: DiffEntry[]): AgentRunSummary {
  const completedSteps = steps.filter(
    (step) => step.status === "done" || step.status === "skipped"
  ).length;
  const totalSteps = steps.length;
  const activeStep = steps.find((step) => step.status === "doing") ?? null;
  const pendingChanges = diffs.filter((diff) =>
    REVIEWABLE_DIFF_STATUSES.has(diff.status)
  ).length;

  return {
    activeStep,
    completedSteps,
    nextStep:
      activeStep ??
      steps.find((step) => step.status === "error") ??
      steps.find((step) => step.status === "todo") ??
      null,
    pendingChanges,
    progressPercent: totalSteps === 0 ? 0 : Math.round((completedSteps / totalSteps) * 100),
    reviewRequired: pendingChanges > 0,
    totalSteps,
  };
}

export function agentStateLabel(state: AgentState) {
  const labels: Record<AgentState, string> = {
    idle: "Ready",
    thinking: "Understanding task",
    planning: "Building plan",
    acting: "Working",
    reviewing: "Reviewing changes",
    waiting_user: "Needs review",
    done: "Completed",
    error: "Needs attention",
  };
  return labels[state];
}

export function isAgentBusy(state: AgentState) {
  return ["thinking", "planning", "acting", "reviewing"].includes(state);
}
