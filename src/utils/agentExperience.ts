import type { AgentState, DiffEntry, Step } from "../types/agent";
import type { MessageKey } from "../i18n/messages";

/**
 * 还等着人处理的改动状态 —— 对应后端的 `is_reviewable_diff_status`。
 *
 * 前端以前有三份：这里一份、`useAgentStore` 一份、`DiffView` 里还有一个
 * `isReviewableDiffStatus`。三份都写着同样的三个状态，所以加一个新状态时会有两处忘改，
 * 而症状是"审查区说没有待办、状态栏说等你处理"这种对不上的界面。
 */
const REVIEWABLE_DIFF_STATUSES = new Set<DiffEntry["status"]>([
  "pending",
  "partial",
  "failed",
]);

export function isReviewableDiffStatus(status: DiffEntry["status"]): boolean {
  return REVIEWABLE_DIFF_STATUSES.has(status);
}

export function isReviewableDiff(diff: DiffEntry): boolean {
  return isReviewableDiffStatus(diff.status);
}

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
  const pendingChanges = diffs.filter(isReviewableDiff).length;

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

/**
 * 状态行第二句：现在到底在等什么。
 *
 * **出错排在最前面。** 一次因为上下文超限失败的运行，以前会显示成「接下来：Create ...」——
 * 计划里还留着没跑的步骤，而那次运行已经死了；用户看到的是一句"下一步要做什么"，
 * 完全不知道自己该干什么。失败的时候唯一有用的信息是"它停了，原因在对话里"。
 *
 * **只有真的挂着一道问题时才说"在对话里等你回答"。** 以前这句话只看 `state ===
 * "waiting_user"`，而那个状态覆盖两件完全不同的事：模型问了你一道题，和这一轮跑完了、
 * 改动等你审。改动全处理完之后第一个分支不成立，就掉到这句上，于是界面断言对话里有人
 * 在等 —— 而对话里什么都没有。一次被中断的会话恢复出来也是 `waiting_user`
 * （见 `normalizeRestoredAgentState`），同样会撞上这句。
 *
 * 问题不在挂着时不另编一句"没事了"：那同样是猜。落回模式那一行 —— 它永远是真的。
 */
export function runDetailMessage(input: {
  state: AgentState;
  summary: AgentRunSummary;
  hasPendingQuestion: boolean;
  ideModeLabel: string;
  modeLabel: string;
}): { key: MessageKey; params?: Record<string, string | number> } {
  const { state, summary, hasPendingQuestion, ideModeLabel, modeLabel } = input;
  if (state === "error") {
    return { key: "summary.detail.failed" };
  }
  if (summary.reviewRequired) {
    return summary.pendingChanges === 1
      ? { key: "summary.detail.review.one" }
      : { key: "summary.detail.review.many", params: { count: summary.pendingChanges } };
  }
  if (hasPendingQuestion) {
    return { key: "summary.detail.waiting" };
  }
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
