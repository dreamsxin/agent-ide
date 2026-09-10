/**
 * 金额换算。界面上用美元，后端存整数"微美元"（1 USD = 1_000_000 micros）。
 *
 * 用整数而不是浮点：钱的累加和比较不该带浮点误差，而且这些值最终要拿去卡住
 * 一次运行的花费，差一点就是判断错。
 */

/** 微美元换算基数 */
export const MICROS_PER_USD = 1_000_000;

/**
 * 把界面上的美元字符串转成整数微美元。
 *
 * 故意做字符串解析而不是 `Number(value) * 1_000_000`：后者对 "0.29" 会算出
 * 289999.99999999994，取整方式一变金额就差一点。超过 6 位小数的部分直接截掉，
 * 微美元已经是能表示的最小单位。
 *
 * 返回 `undefined` 表示"没填"：空串、非法输入、以及 0 都归到这一档，和后端
 * 把 `maxRunTokens: 0` 当作未设置保持一致——手写的一个 0 不该锁死所有运行。
 */
export function usdToMicros(value: string): number | undefined {
  const trimmed = value.trim();
  if (!trimmed || !/^\d*\.?\d*$/.test(trimmed)) return undefined;
  const [whole = "", fraction = ""] = trimmed.split(".");
  const micros =
    Number(whole || "0") * MICROS_PER_USD + Number(fraction.slice(0, 6).padEnd(6, "0") || "0");
  return micros > 0 ? micros : undefined;
}

/** 微美元转回界面用的美元字符串，去掉末尾多余的 0 */
export function microsToUsdInput(micros?: number): string {
  if (!micros) return "";
  const whole = Math.floor(micros / MICROS_PER_USD);
  const fraction = String(micros % MICROS_PER_USD)
    .padStart(6, "0")
    .replace(/0+$/, "");
  return fraction ? `${whole}.${fraction}` : String(whole);
}

/**
 * 展示用的金额格式化，`$0.0234`。
 *
 * 刻意逐位复制后端 `format_micros_usd` 的整数算法（截断，不四舍五入）：同一笔花费
 * 在状态栏和 action log 里必须是同一个字符串，差一位会让人以为看到了两笔账。
 * 固定 4 位小数的理由和后端一样 —— 单次运行常常远小于 1 分钱，2 位会全变成
 * `$0.00`，看着像没花钱。
 */
export function formatMicrosUsd(micros: number): string {
  const whole = Math.floor(micros / MICROS_PER_USD);
  const fraction = Math.floor((micros % MICROS_PER_USD) / 100);
  return `$${whole}.${String(fraction).padStart(4, "0")}`;
}

/**
 * 金额上限当前是否真的生效。
 *
 * `no_price` 这一档必须单独存在：填了上限但价格不全时后端不会执行它（只有一半
 * 价格的估算会系统性低估花费），界面上不说清楚，用户就会以为自己已经有了成本
 * 保护。
 */
export function spendCapStatus(
  promptPrice: string,
  completionPrice: string,
  cap: string
): "active" | "no_price" | "off" {
  if (usdToMicros(cap) === undefined) return "off";
  if (usdToMicros(promptPrice) === undefined || usdToMicros(completionPrice) === undefined) {
    return "no_price";
  }
  return "active";
}
