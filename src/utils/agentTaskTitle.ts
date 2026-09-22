/** 任务标题的最大长度；超过就截断并加省略号 */
const MAX_TASK_TITLE_CHARS = 60;
/** 截断后保留的字符数，留 3 个位置给省略号 */
const TRUNCATED_TITLE_CHARS = MAX_TASK_TITLE_CHARS - 3;

/**
 * 从这一轮的 prompt 推出任务标题。
 *
 * 取第一行而不是整段：多行 prompt 的后面几行通常是约束和示例，标题栏放不下，
 * 而第一行几乎总是"要做什么"。空白折叠成单个空格 —— 粘贴进来的文本常带缩进，
 * 原样显示会在标题里留一段看不出原因的空隙。
 *
 * 拿不出任何文字时返回空串，由调用方决定显示什么；这里不编一个假标题。
 */
export function deriveTaskTitle(prompt: string): string {
  const firstLine = prompt
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line.length > 0);
  if (!firstLine) return "";

  const collapsed = firstLine.replace(/\s+/g, " ");
  if (collapsed.length <= MAX_TASK_TITLE_CHARS) return collapsed;
  return `${collapsed.slice(0, TRUNCATED_TITLE_CHARS)}...`;
}
