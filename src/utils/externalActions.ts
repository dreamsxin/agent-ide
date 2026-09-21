/**
 * 撤不回的外部动作（浏览器导航等）在前端的规范化与展示。
 *
 * 单独一个纯模块：这些值从 Tauri 命令回来，是运行时数据，必须 normalize 而不是
 * `as` —— 旧版本后端、被改过的会话都可能给出缺字段的对象，而这份记录正是一个
 * 撤不回的能力唯一的补偿，渲染成 `undefined` 等于把它悄悄丢了。
 */

/** 后端 `ExternalActionRecord` 的前端对应物。 */
export interface ExternalActionRecord {
  id: string;
  timestamp: string;
  kind: string;
  target: string;
  detail: string;
  runId: string | null;
  /**
   * 这条是上一次会话留下的，从磁盘恢复出来的。
   *
   * 后端给这个标，而不是让界面拿时间戳去算：`runId` 只能区分"哪一次运行"，区分不了
   * "哪一次会话"—— 重启之后 `runId` 仍然是个陌生 id，而"这是刚刚发生的"和"这是上周
   * 那次留下的"对用户完全不是一回事。
   */
  restored: boolean;
}

function stringField(source: Record<string, unknown>, key: string): string {
  const value = source[key];
  return typeof value === "string" ? value : "";
}

/**
 * 把后端/存储里的值收成记录数组。
 *
 * 丢弃条目的唯一标准是"连 kind 和 target 都没有"—— 那种条目在界面上是一行空白，
 * 比不显示更糟。其余缺失字段补空串，宁可显示得不完整，也不要整条消失。
 */
export function normalizeExternalActions(value: unknown): ExternalActionRecord[] {
  if (!Array.isArray(value)) return [];
  const records: ExternalActionRecord[] = [];
  value.forEach((item, index) => {
    if (typeof item !== "object" || item === null) return;
    const source = item as Record<string, unknown>;
    const kind = stringField(source, "kind");
    const target = stringField(source, "target");
    if (!kind && !target) return;
    const runId = source.runId;
    records.push({
      // id 缺失时用下标兜底：React key 冲突会让两条记录只显示一条
      id: stringField(source, "id") || `external-${index}`,
      timestamp: stringField(source, "timestamp"),
      kind,
      target,
      detail: stringField(source, "detail"),
      runId: typeof runId === "string" && runId.length > 0 ? runId : null,
      // 只有后端明确说 true 才是 true：缺字段（旧后端）当成"这次会话的"，因为那是
      // 两者里更不容易误导人的一边 —— 把刚刚发生的事标成历史，用户会以为它没发生
      restored: source.restored === true,
    });
  });
  return records;
}

/**
 * 这条记录是没有真的发生的尝试：被拒、失败，或者被 Stop 拦下。
 *
 * `_cancelled` 必须算在里面。漏掉它的时候，一次被 Stop 拦住的导航会被计进
 * "N browser action(s) — cannot be undone" —— 在这个产品唯一承诺可信的地方
 * 说一件没发生的事。
 */
export function isRefusedAction(action: ExternalActionRecord): boolean {
  return (
    action.kind.endsWith("_refused") ||
    action.kind.endsWith("_failed") ||
    action.kind.endsWith("_cancelled")
  );
}

/**
 * 这条记录属于**别的**运行。
 *
 * 后端的列表跨运行保留（导航已经发生，不该因为换了个提问就消失），所以界面必须能
 * 说出哪些不是当前这次干的 —— 否则用户会把上一次运行打开的站点当成刚发生的事。
 * 任一边缺 id 时不下判断：标错来源比不标更糟。
 */
export function isFromOtherRun(
  action: ExternalActionRecord,
  currentRunId: string | null
): boolean {
  if (!action.runId || !currentRunId) return false;
  return action.runId !== currentRunId;
}

/**
 * 一行摘要。
 *
 * 类别写在前面而不是翻译成句子：`browser_open_refused` 和 `browser_open` 差一个词，
 * 用户扫一列的时候需要它们看起来不一样。
 */
export function describeExternalAction(action: ExternalActionRecord): string {
  const target = action.target || "(unknown target)";
  return action.detail ? `${target} — ${action.detail}` : target;
}

/**
 * 清空动作在日志里留下的墓碑 kind，和后端的 `LOG_CLEARED_KIND` 对齐。
 *
 * 它不是一次外部动作：既没有出网也没有截屏。算进"已经发生 N 次"会让那个数字不再可信，
 * 而那个数字是这块 UI 唯一的作用。
 */
export const LOG_CLEARED_KIND = "external_log_cleared";

/** 这条只是日志自己的记账，不是 Agent 做的事。 */
export function isBookkeepingAction(action: ExternalActionRecord): boolean {
  return action.kind === LOG_CLEARED_KIND;
}

/** 有几次真的发生了，有几次被拒/失败。给标题用。 */
export function summarizeExternalActions(actions: ExternalActionRecord[]): {
  performed: number;
  refused: number;
} {
  const real = actions.filter((action) => !isBookkeepingAction(action));
  const refused = real.filter(isRefusedAction).length;
  return { performed: real.length - refused, refused };
}
