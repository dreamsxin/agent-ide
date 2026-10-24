import { describe, expect, it } from "vitest";
import { DEFAULT_RESERVED_OUTPUT_TOKENS, estimateInputTokens } from "./contextBudget";

describe("estimateInputTokens", () => {
  /**
   * 窗口未知就说未知。
   *
   * 这一条是被一句反问改掉的设计：之前这里假定 128k，而各家主力已经是 200k 到 1M —— 那个
   * 常量在多数情况下都偏小，于是估算开始报假警。假警报和假信心一样坏。
   */
  it("refuses to guess a window", () => {
    expect(estimateInputTokens(undefined, undefined, undefined)).toBeUndefined();
    expect(estimateInputTokens(undefined, 4_096, 8_192)).toBeUndefined();
  });

  /** 百万窗口要原样算出来，不能被任何上限夹住 */
  it("handles a million-token window", () => {
    expect(estimateInputTokens(1_000_000, undefined, undefined)).toBe(
      1_000_000 - DEFAULT_RESERVED_OUTPUT_TOKENS - 512
    );
  });

  it("uses the window you configured", () => {
    expect(estimateInputTokens(200_000, 64_000, undefined)).toBe(200_000 - 64_000 - 512);
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
   * 保存前后同一份配置显示成两个数字，所以钉住一个两边共有的结果。
   */
  it("matches the backend for the same inputs", () => {
    expect(estimateInputTokens(128_000, undefined, undefined)).toBe(123_392);
  });
});
