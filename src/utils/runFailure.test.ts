import { describe, expect, it } from "vitest";
import { runFailureHint } from "./runFailure";
import { translate } from "../i18n";

describe("失败原因 → 可操作提示", () => {
  /**
   * 用户报告的那一次：会话因为上下文超限失败，界面只写着"等你处理"。
   * 提供方的原话说的是发生了什么，不说该怎么办 —— 这句提示补的就是"怎么办"。
   */
  it("上下文超限认得出来", () => {
    const real =
      "This model's maximum context length is 128000 tokens, however you requested 132518 tokens (130518 in the messages, 2000 in the completion).";
    expect(runFailureHint(real)).toBe("failure.hint.contextLimit");
    expect(translate("zh", "failure.hint.contextLimit")).toContain("压缩模式");
  });

  it("Anthropic 那边的措辞也认", () => {
    expect(runFailureHint("prompt is too long: 215000 tokens > 200000 maximum")).toBe(
      "failure.hint.contextLimit"
    );
  });

  it("key、限流、额度、网络各归各类", () => {
    expect(runFailureHint("401 Unauthorized: Incorrect API key provided")).toBe(
      "failure.hint.auth"
    );
    expect(runFailureHint("Rate limit reached for gpt-4o")).toBe("failure.hint.rateLimit");
    expect(runFailureHint("You exceeded your current quota (insufficient_quota)")).toBe(
      "failure.hint.quota"
    );
    expect(runFailureHint("error sending request: connection reset by peer")).toBe(
      "failure.hint.network"
    );
  });

  /**
   * 认不出来就不给提示。编一句"请检查配置"看起来像诊断，实际什么都没说，
   * 而原话仍然在下面 —— 那是唯一准确的信息。
   */
  it("认不出来的失败不编提示", () => {
    expect(runFailureHint("Stage Implement failed: tool protocol violation")).toBeNull();
    expect(runFailureHint("")).toBeNull();
    expect(runFailureHint(null)).toBeNull();
  });
});
