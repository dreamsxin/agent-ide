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

/**
 * 状态 → 文案键。返回键而不是句子，因为句子有两种语言。
 *
 * 这里曾经直接返回英文标签，而 `TaskView` 又渲染枚举值本身，于是同一个状态在界面上有
 * 三种写法（"Needs review" / "waiting_user" / 中文）。映射只留这一份，翻译交给 `t()`。
 */
export function agentStateMessageKey(state: AgentState) {
  return `state.${state}` as const;
}


export function isAgentBusy(state: AgentState) {
  return ["thinking", "planning", "acting", "reviewing"].includes(state);
}
