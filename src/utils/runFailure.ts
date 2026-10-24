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

/**
 * 后端截断诊断里固定出现的这段话，来源是 `agent/executor.rs` 的 `CUT_OFF_MARKER`。
 *
 * 两份字面量必须一字不差。刻意不匹配 "cut off" 这个短语：输出预算那条原话里也有
 * "the output was cut off at the output limit"，两类的建议不一样（一个是答案空的，
 * 一个是改动一个都没生成）。也必须排在 contextLimit 前面 —— 这句话自己就含
 * "fit the context window"，排在后面会被归成上下文超限，给出相反的建议。
 */
const CUT_OFF_MARKER = "ended before the block closed";

const HINTS: { key: MessageKey; markers: string[] }[] = [
  {
    // 推理模型把整个输出预算花在思考上：content 是空的、finish_reason 是 length。
    // 这句话是后端自己写的（`llm_client::empty_response_error`），措辞稳定。
    // 它必须排在上下文超限前面：两者都在说 token，但一个要调小输入、一个要调大输出，
    // 指错方向比不给提示更糟。
    key: "failure.hint.outputCap",
    markers: [
      "no message content and no tool calls",
      "finish_reason=length",
      "spent the whole output budget on reasoning",
    ],
  },
  {
    // 回答写到一半被截断、代码块没收尾。措辞与顺序约束见 CUT_OFF_MARKER 的注释。
    key: "failure.hint.cutOff",
    markers: [CUT_OFF_MARKER],
  },
  // 应用改动被拒绝的三类原话（`agent/diff_apply.rs`）。它们排在提供方错误前面：
  // 这几句是我们自己写的，措辞稳定，不会和模型报错撞词。
  {
    // 锚点对不上和 baseHash 对不上是同一件事的两种表现：diff 是照旧文件生成的。
    // 这一条必须排在最前：它的原话后面跟着 hunk 的前 200 个字符，那段模型生成的
    // 文本里完全可能出现别的 marker，先匹配到别人就会给出一句不对的建议。
    key: "failure.hint.staleDiff",
    markers: [
      "file changed since diff was generated",
      "basehash",
      "original content is empty",
      "original content matched more than once",
      "original content not found",
    ],
  },
  {
    key: "failure.hint.fileExists",
    markers: ["refusing to overwrite existing file"],
  },
  {
    key: "failure.hint.missingFile",
    markers: ["file not found"],
  },
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

/** 一次应用里最多在横幅上写几条原话；再多就只给个数字，横幅不能长成一屏。 */
const MAX_LISTED_FAILURES = 3;

/**
 * 把"哪些改动没应用上、为什么"拼成横幅要显示的那段话。
 *
 * 以前三处写的是 `Failed to apply N diff(s).` / `Failed to apply diff.` —— 后端
 * 明明给了 `file` 和 `message`（"Refusing to overwrite existing file: ..."），
 * 横幅把它们全丢掉，只留一个数字。用户报告的就是这个："报错了，只写 Failed to
 * apply diff.，不知道为什么"。
 *
 * 这里不翻译也不改写原话：原话是唯一准确的信息，可操作的建议由 `runFailureHint`
 * 在它下面另起一行给。拼出来的是 `文件: 原话`，两边都是数据，不是需要翻译的文案。
 */
export function applyFailureSummary(
  failed: { file: string; message: string }[]
): string | null {
  if (failed.length === 0) return null;
  const lines = failed
    .slice(0, MAX_LISTED_FAILURES)
    .map((failure) => `${failure.file}: ${failure.message}`);
  const hidden = failed.length - lines.length;
  if (hidden > 0) {
    lines.push(`(+${hidden})`);
  }
  return lines.join("\n");
}

