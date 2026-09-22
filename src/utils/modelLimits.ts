/**
 * 模型的窗口和输出上限。
 *
 * **按模型 id 匹配，不按供应商。** 这是这张表唯一正确的形状：同一个模型会被好几个端点转发
 * （官方、Azure、各种网关），而同一家自己的模型之间上限能差两个数量级 —— 一个 vision flash
 * 变体只允许 1k 输出，旗舰允许 128k。按供应商给默认值必然在其中一边是错的。
 *
 * 这些值只是**填进输入框的起点**，不是运行时的隐藏兜底：用户看得见、改得动，而且
 * `Max output` 会真的发给供应商，所以一个过大的值会换来一次明确的 400，而不是静默截断。
 *
 * 匹配是有序的，**先具体后笼统**（`gpt-5.4-mini` 必须排在 `gpt-5` 之前）。没有匹配就是
 * `undefined` —— 界面照这个事实说"未知，请填"，不猜。各家一年里改过多次窗口，猜出来的数字
 * 会让旁边那个百分比看起来可信。
 */
export interface ModelLimits {
  contextWindow: number;
  maxOutput: number;
}

interface Rule {
  pattern: RegExp;
  limits: ModelLimits;
}

const RULES: Rule[] = [
  // OpenAI：mini / nano / codex 的窗口比旗舰小，必须排在前面
  { pattern: /gpt-5[.\d]*-(?:mini|nano|codex)/, limits: { contextWindow: 400_000, maxOutput: 128_000 } },
  { pattern: /gpt-(?:6|5)/, limits: { contextWindow: 1_050_000, maxOutput: 128_000 } },
  { pattern: /gpt-4\.1/, limits: { contextWindow: 1_000_000, maxOutput: 32_768 } },
  { pattern: /gpt-4o/, limits: { contextWindow: 128_000, maxOutput: 16_384 } },
  // 老一代仍然挂在预设的下拉里。不给它们规则的后果是选中就把两个框清空 —— 那比给一个
  // 保守的旧数字更糟：估算变成 unknown，而 Max output 干脆不发了。
  { pattern: /gpt-4-turbo/, limits: { contextWindow: 128_000, maxOutput: 4_096 } },
  { pattern: /gpt-4-32k/, limits: { contextWindow: 32_768, maxOutput: 4_096 } },
  { pattern: /gpt-3\.5-turbo|gpt-35-turbo/, limits: { contextWindow: 16_385, maxOutput: 4_096 } },
  { pattern: /gpt-4/, limits: { contextWindow: 8_192, maxOutput: 4_096 } },

  // Anthropic：4.5 haiku 和 3.x 系列的输出上限远低于 5.x
  { pattern: /claude-(?:fable|mythos|opus|sonnet)-5/, limits: { contextWindow: 1_000_000, maxOutput: 128_000 } },
  { pattern: /claude-haiku-4[.\-]5/, limits: { contextWindow: 200_000, maxOutput: 64_000 } },
  // opus-4 的输出上限只有 sonnet-4 的一半，必须单独一条 —— 合在一起就是这张表本来要消灭的
  // 那个错误，而且 Max output 会真的发出去，多填一倍换来的是一次 400
  { pattern: /claude-opus-4/, limits: { contextWindow: 200_000, maxOutput: 32_000 } },
  { pattern: /claude-sonnet-4/, limits: { contextWindow: 200_000, maxOutput: 64_000 } },
  { pattern: /claude-3-5-sonnet/, limits: { contextWindow: 200_000, maxOutput: 8_192 } },
  { pattern: /claude-3-(?:opus|sonnet|haiku)/, limits: { contextWindow: 200_000, maxOutput: 4_096 } },


  // DeepSeek：V4 一代是百万窗口，旧的别名仍然是 128k
  { pattern: /deepseek-v4/, limits: { contextWindow: 1_000_000, maxOutput: 384_000 } },
  { pattern: /deepseek-reasoner/, limits: { contextWindow: 128_000, maxOutput: 65_536 } },
  { pattern: /deepseek-chat/, limits: { contextWindow: 128_000, maxOutput: 8_192 } },

  // 其它常见自建/网关模型
  { pattern: /glm-5/, limits: { contextWindow: 200_000, maxOutput: 64_000 } },
  { pattern: /glm-4\.[67]/, limits: { contextWindow: 200_000, maxOutput: 131_072 } },
  { pattern: /kimi-k3|(?:^|[^\w])k3(?:[^\w]|$)/, limits: { contextWindow: 1_048_576, maxOutput: 131_072 } },
  { pattern: /kimi-k2/, limits: { contextWindow: 262_144, maxOutput: 98_304 } },
  { pattern: /qwen3[.\d]*-(?:max|plus|flash)/, limits: { contextWindow: 1_000_000, maxOutput: 131_072 } },
];

/**
 * 查这个模型的窗口和输出上限。查不到返回 `undefined`。
 *
 * 大小写和厂商前缀（`openai/gpt-4o`、`deepseek-ai/DeepSeek-V4`）都要能认：网关普遍在 id
 * 前面挂一段自己的命名空间，只做全等匹配等于对网关用户完全失效。
 */
export function modelLimits(model: string): ModelLimits | undefined {
  const id = model.trim().toLowerCase();
  if (id === "") return undefined;
  return RULES.find((rule) => rule.pattern.test(id))?.limits;
}
