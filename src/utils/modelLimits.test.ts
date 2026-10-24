import { describe, expect, it } from "vitest";
import { modelLimits } from "./modelLimits";

describe("modelLimits", () => {
  /**
   * 表的形状：按**模型**匹配，不按供应商。同一家自己的模型之间上限能差两个数量级，
   * 所以"给这一家一个默认值"必然在其中一边是错的。
   */
  it("keeps families apart inside one vendor", () => {
    expect(modelLimits("claude-3-opus-20240229")?.maxOutput).toBe(4_096);
    expect(modelLimits("claude-3-5-sonnet-20241022")?.maxOutput).toBe(8_192);
    expect(modelLimits("claude-haiku-4-5")?.maxOutput).toBe(64_000);
    expect(modelLimits("claude-sonnet-5")?.maxOutput).toBe(128_000);
  });

  /** 更具体的规则必须先匹配，否则 mini 会拿到旗舰的窗口 */
  it("matches the more specific rule first", () => {
    expect(modelLimits("gpt-5.4-mini")?.contextWindow).toBe(400_000);
    expect(modelLimits("gpt-5.4")?.contextWindow).toBe(1_050_000);
    expect(modelLimits("gpt-4o-mini")?.contextWindow).toBe(128_000);
    // 老一代要拿到自己的值，而不是被 `gpt-4` 那条笼统规则或 4o 的值盖掉
    expect(modelLimits("gpt-4-turbo")).toEqual({ contextWindow: 128_000, maxOutput: 4_096 });
    expect(modelLimits("gpt-4-32k")?.contextWindow).toBe(32_768);
    expect(modelLimits("gpt-4")?.contextWindow).toBe(8_192);
    expect(modelLimits("gpt-35-turbo")?.contextWindow).toBe(16_385);
  });

  /**
   * opus-4 的输出上限只有 sonnet-4 的一半。合成一条规则就是这张表本来要消灭的那个错误，
   * 而 Max output 会真的发出去 —— 多填一倍换来的是一次 400。
   */
  it("does not give opus-4 the sonnet-4 output cap", () => {
    expect(modelLimits("claude-opus-4-1")?.maxOutput).toBe(32_000);
    expect(modelLimits("claude-sonnet-4-5")?.maxOutput).toBe(64_000);
  });


  /**
   * 网关普遍在 id 前面挂一段自己的命名空间，大小写也不统一。只做全等匹配等于对网关用户
   * 完全失效 —— 而那恰好是最需要这张表的一群人（他们那边没有预设可选）。
   */
  it("sees through gateway prefixes and casing", () => {
    expect(modelLimits("openai/gpt-4o")?.maxOutput).toBe(16_384);
    expect(modelLimits("deepseek-ai/DeepSeek-V4-Flash")?.maxOutput).toBe(384_000);
  });

  /** 旧别名不能拿到新一代的百万窗口 */
  it("does not promote an old alias to the new generation", () => {
    expect(modelLimits("deepseek-chat")).toEqual({ contextWindow: 128_000, maxOutput: 8_192 });
    expect(modelLimits("deepseek-v4-flash")?.contextWindow).toBe(1_000_000);
  });

  /**
   * 查不到就是查不到。返回一个猜出来的数字比返回 undefined 更糟：界面会把它显示成一个
   * 看着可信的百分比，而用户没有任何理由怀疑它。
   */
  it("returns undefined rather than guessing", () => {
    expect(modelLimits("")).toBeUndefined();
    expect(modelLimits("my-finetune-v3")).toBeUndefined();
    expect(modelLimits("llama-3-70b-instruct")).toBeUndefined();
  });
});
