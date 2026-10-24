/**
 * Token 数量输入框的解析与显示。
 *
 * 为什么不用 `<input type="number">` + `Number()`：那种框里打不出 `128k`，也打不出
 * `128,000` —— 浏览器直接拒收字母和逗号。于是"上下文窗口填多少"这件事变成了数零：
 * 128000 和 1280000 在 11px 的输入框里看起来几乎一样，而后者会让预算估算凭空多出一个数量级。
 *
 * 所以：文本框 + 这里解析，并且把解析结果回显出来（`formatTokenCount`），让用户自己确认
 * 那个数是他想的那个。
 */

/** 千分位和下划线只是给人看的，解析时先去掉 */
const GROUPING = /[,_\s]/g;

/**
 * 把用户输入解析成 token 数。
 *
 * 认三种写法：`128000`、`128k`、`0.5m`。认不出来返回 `undefined` —— 和空输入同一个结果，
 * 因为界面对两者的话术是一样的（"没设"），而猜一个数字比不设更危险。
 *
 * `0` 也是 `undefined`：后端把 0 当"没设"，界面必须照这个事实回显，否则用户以为自己
 * 设了一个"上限为 0"的限制。
 */
export function parseTokenInput(raw: string): number | undefined {
  const cleaned = raw.replace(GROUPING, "").trim().toLowerCase();
  if (cleaned === "") return undefined;

  const match = /^(\d+(?:\.\d+)?)([km])?$/.exec(cleaned);
  if (!match) return undefined;

  const magnitude = match[2] === "k" ? 1_000 : match[2] === "m" ? 1_000_000 : 1;
  const value = Math.floor(Number(match[1]) * magnitude);
  if (!Number.isFinite(value) || value <= 0) return undefined;
  return value;
}

/**
 * 回显解析结果。
 *
 * 带千分位：数零正是这个输入框最容易出错的地方。解析不出来时说"not set"而不是留空 ——
 * 留空和"正在输入"长得一样，而这里要回答的恰恰是"我刚打的这串到底被认成了什么"。
 */
export function formatTokenCount(value: number | undefined): string {
  return value === undefined ? "not set" : value.toLocaleString();
}
