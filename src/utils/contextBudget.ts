/**
 * 上下文预算的估算规则。
 *
 * 逐位对应后端 `services/context.rs` 里的三个常量和
 * `LlmProfile::effective_input_tokens`：设置面板要在用户还没保存之前就显示这一行，所以这条
 * 公式必须在前端也有一份。两边不一致的后果是保存前后同一份配置显示成两个数字。
 */

/** 对应 Rust `DEFAULT_RESERVED_OUTPUT_TOKENS` */
export const DEFAULT_RESERVED_OUTPUT_TOKENS = 4_096;

/** 对应 Rust `CONTEXT_ASSEMBLY_HEADROOM_TOKENS` */
export const CONTEXT_ASSEMBLY_HEADROOM_TOKENS = 512;

/**
 * 对应 Rust `ASSUMED_MAX_CONTEXT_TOKENS`。
 *
 * 没填窗口时估算按它算，并且在界面上标成 "assumed"。估低是安全方向：预算显得更紧，
 * 不会让人以为还有余量。刻意不做 per-model 表 —— 各家窗口改得比这个仓库快。
 */
export const ASSUMED_MAX_CONTEXT_TOKENS = 128_000;

/**
 * 有效输入预算：窗口 − 预留输出 − 装配余量。
 *
 * 三个入参都是输入框里的原始字符串（可能是 `128k`、可能是空）。任何一个空着都不影响出数字，
 * 这正是这个函数存在的意义：用户问"这三个能不能有默认"，答案是两个本来就有，只是界面从没说。
 */
export function estimateInputTokens(
  maxContext: number | undefined,
  reservedOutput: number | undefined,
  maxOutput: number | undefined
): number {
  const context = maxContext ?? ASSUMED_MAX_CONTEXT_TOKENS;
  // 没填预留输出就退回 Max output：那才是这个模型真正会从窗口里占掉的部分
  const reserved = reservedOutput ?? maxOutput ?? DEFAULT_RESERVED_OUTPUT_TOKENS;
  return Math.max(0, context - reserved - CONTEXT_ASSEMBLY_HEADROOM_TOKENS);
}
