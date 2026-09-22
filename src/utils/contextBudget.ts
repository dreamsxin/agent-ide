/**
 * 上下文预算的估算规则。
 *
 * 逐位对应后端 `services/context.rs` 的常量和 `LlmProfile::effective_input_tokens`：设置面板
 * 要在用户还没保存之前就显示这一行，所以这条公式必须在前端也有一份。两边不一致的后果是保存
 * 前后同一份配置显示成两个数字。
 *
 * **不假定窗口。** 曾经这里有一个全局 `ASSUMED_MAX_CONTEXT_TOKENS = 128_000`，被一句话点破：
 * 现在各家主力是 200k 到 1M，一个固定常量在多数情况下都偏小，于是这一行开始报假警 —— 说预算
 * 只有十几万而实际有一百万。窗口未知时正确的显示是"未知"，而不是一个看着可信的错数字。
 * 界面的兜底是它**已经知道**的东西：用户选的那个供应商预设里带的窗口。
 */

/** 对应 Rust `DEFAULT_RESERVED_OUTPUT_TOKENS`。这是"给回答留多少"的策略值，和模型无关。 */
export const DEFAULT_RESERVED_OUTPUT_TOKENS = 4_096;

/** 对应 Rust `CONTEXT_ASSEMBLY_HEADROOM_TOKENS` */
export const CONTEXT_ASSEMBLY_HEADROOM_TOKENS = 512;

/**
 * 有效输入预算：窗口 − 预留输出 − 装配余量。
 *
 * 窗口未知返回 `undefined`：调用方要照这个事实措辞（"填上 Max context 才能算"），不能显示 0，
 * 也不能显示一个猜出来的数。预留输出未知则按默认算，那一项猜得起。
 */
export function estimateInputTokens(
  maxContext: number | undefined,
  reservedOutput: number | undefined,
  maxOutput: number | undefined
): number | undefined {
  if (maxContext === undefined) return undefined;
  // 没填预留输出就退回 Max output：那才是这个模型真正会从窗口里占掉的部分
  const reserved = reservedOutput ?? maxOutput ?? DEFAULT_RESERVED_OUTPUT_TOKENS;
  return Math.max(0, maxContext - reserved - CONTEXT_ASSEMBLY_HEADROOM_TOKENS);
}
