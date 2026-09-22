import { describe, expect, it } from "vitest";
import {
  ASSUMED_MAX_CONTEXT_TOKENS,
  DEFAULT_RESERVED_OUTPUT_TOKENS,
  estimateInputTokens,
} from "./contextBudget";

describe("estimateInputTokens", () => {
  /**
   * 用户的问题是"这三个数值能不能有默认"。这条断言就是答案：一个都不填也出数字，
   * 而以前窗口一空整行就显示 "not set"，看起来像"不填不能用"。
   */
  it("produces a budget with nothing configured", () => {
    expect(estimateInputTokens(undefined, undefined, undefined)).toBe(
      ASSUMED_MAX_CONTEXT_TOKENS - DEFAULT_RESERVED_OUTPUT_TOKENS - 512
    );
  });

  it("uses the window you configured instead of the assumption", () => {
    expect(estimateInputTokens(32_000, 4_096, undefined)).toBe(32_000 - 4_096 - 512);
  });

  /** 没填预留输出时退回 Max output：那才是这个模型真正会从窗口里占掉的部分 */
  it("falls back to max output when reserved output is empty", () => {
    expect(estimateInputTokens(32_000, undefined, 8_192)).toBe(32_000 - 8_192 - 512);
  });

  /** 预留比窗口还大不能变成负数 —— 那会渲染成一个带负号的预算 */
  it("never goes negative", () => {
    expect(estimateInputTokens(1_000, 999_999, undefined)).toBe(0);
  });

  /**
   * 这份公式在 Rust 里也有一份（`LlmProfile::effective_input_tokens`）。数值对不上的后果是
   * 保存前后同一份配置显示成两个数字，所以这里钉住那个共同的结果。
   */
  it("matches the backend's number for an unconfigured profile", () => {
    expect(estimateInputTokens(undefined, undefined, undefined)).toBe(123_392);
  });
});
