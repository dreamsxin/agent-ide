import type { MessageKey } from "../i18n/messages";

/**
 * 把一条运行失败的原话认成一类"你可以做点什么"的提示。
 *
 * 后端把提供方的报错原样传上来 —— 那是唯一准确的信息，不能改写也不能兜成"运行失败"。
 * 但原话只说发生了什么（`This model's maximum context length is 128000 tokens...`），
 * 不说该怎么办；用户报告的正是这一点："只写等你处理，不知道如何做"。
 *
 * 所以这里**不替换**原话，只在它上面加一句可操作的提示，而且只在认得出来的时候加。
 * 认不出来就返回 `null`：编一句"请检查你的配置"比不说更糟，它看起来像诊断但什么都没说。
 *
 * 匹配用的是提供方错误里稳定的那几个词（OpenAI / Anthropic / 兼容端点大都一致），
 * 全部转小写后匹配。刻意不匹配 HTTP 状态码数字本身：`429` 这种三位数会在文件名、
 * 端口、token 数里误命中。
 */
export type RunFailureHint = MessageKey | null;

const HINTS: { key: MessageKey; markers: string[] }[] = [
  {
    // 上下文超限：这是最常见的一类，也是最需要提示的 —— 它的解法全在界面上
    key: "failure.hint.contextLimit",
    markers: [
      "maximum context length",
      "context_length_exceeded",
      "context length exceeded",
      "context window",
      "too many tokens",
      "reduce the length of the messages",
      "prompt is too long",
    ],
  },
  {
    key: "failure.hint.auth",
    markers: [
      "invalid api key",
      "incorrect api key",
      "unauthorized",
      "authentication_error",
      "invalid_api_key",
    ],
  },
  {
    key: "failure.hint.rateLimit",
    markers: ["rate limit", "rate_limit_exceeded", "too many requests"],
  },
  {
    key: "failure.hint.quota",
    markers: ["insufficient_quota", "exceeded your current quota", "billing"],
  },
  {
    key: "failure.hint.network",
    markers: [
      "connection refused",
      "connection reset",
      "timed out",
      "timeout",
      "dns",
      "failed to lookup address",
      "certificate",
    ],
  },
];

export function runFailureHint(error: string | null): RunFailureHint {
  if (!error) return null;
  const haystack = error.toLowerCase();
  for (const { key, markers } of HINTS) {
    if (markers.some((marker) => haystack.includes(marker))) {
      return key;
    }
  }
  return null;
}
