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


const BUSY_STATES = new Set<AgentState>(["thinking", "planning", "acting", "reviewing"]);

/**
 * Agent 正在自己干活，界面上一切"要它做事"的入口都得先关掉。
 *
 * `waiting_user` **不算**忙：那个状态的意思正好相反 —— 它在等你。一次被中断的会话恢复出来
 * 也是这个状态（见 `normalizeRestoredAgentState`），所以把它算成忙会留下一个解不开的死结：
 * 顶栏的「写代码 / 先做计划」两个按钮一直是禁用的，模式换不回来，而 Agent 根本没在跑。
 * 这个判断曾经在七个地方各写一遍，TopBar 那一份漏了这一条，于是只有它坏。
 */
export function isAgentBusy(state: AgentState) {
  return BUSY_STATES.has(state);
}

/**
 * 还有一次没结束的运行 —— 包括它停下来等你回答的时候。
 *
 * 和 `isAgentBusy` 差的就是 `waiting_user`：那时后端那次运行还挂着，Stop 仍然有意义，
 * 但用户已经可以动界面了。两个问题不同，所以是两个函数，而不是一个带参数的。
 */
export function agentRunIsLive(state: AgentState) {
  return isAgentBusy(state) || state === "waiting_user";
}
