import { describe, expect, it } from "vitest";
import { applyFailureSummary, runFailureHint } from "./runFailure";
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

  /**
   * 用户报告的那一次：推理模型把 4096 的输出预算全花在思考上，content 是空的。
   * 这和上下文超限是相反的解法（调大输出，而不是少发上下文），所以必须分开认。
   */
  it("输出预算被思考吃光不会被当成上下文超限", () => {
    const real =
      "LLM response had no message content and no tool calls. 1 choice(s) returned [choice 0: finish_reason=length, content_chars=0, reasoning_chars=12654, tool_calls=0]. finish_reason=length means the output was cut off at the output limit; a large reasoning_chars with empty content means the model spent the whole output budget on reasoning. This request sent max_tokens=4096; raise the output cap so the model has room for its reasoning *and* an answer.";
    expect(runFailureHint(real)).toBe("failure.hint.outputCap");
    expect(translate("zh", "failure.hint.outputCap")).toContain("Max output");
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

  /**
   * 用户报告的那一次：点 Apply all 报 "Failed to apply diff."，没有原因。
   * 后端拒绝的三类都有出路，提示要说出那条出路，不能只说"失败了"。
   */
  it("应用被拒绝的三类各有一句出路", () => {
    expect(runFailureHint("Refusing to overwrite existing file: D:\\work\\a\\src\\new.ts")).toBe(
      "failure.hint.fileExists"
    );
    expect(runFailureHint("File not found: D:\\work\\a\\src\\gone.ts")).toBe(
      "failure.hint.missingFile"
    );
    expect(
      runFailureHint(
        "File changed since diff was generated for src/a.ts: expected baseHash ab12, got cd34"
      )
    ).toBe("failure.hint.staleDiff");
  });

  /**
   * hunk 锚点对不上的原话后面跟着模型生成的前 200 个字符。那段文本里可能出现
   * 别的 marker（"file not found" 完全可能是被改的那行代码），所以陈旧这一类
   * 必须先匹配 —— 否则给出的建议指向另一件事。
   */
  it("锚点对不上时不被 hunk 内容里的字眼带跑", () => {
    expect(
      runFailureHint(
        'Original content matched more than once in src/a.ts: throw new Error("File not found: x")'
      )
    ).toBe("failure.hint.staleDiff");
  });
});

describe("应用失败的横幅", () => {
  /**
   * 以前这里写的是 `Failed to apply N diff(s).`：后端给了文件名和原因，横幅
   * 只留一个数字。用户看到的就是这句，什么都推断不出来。
   */
  it("后端自己那句话要留在横幅上", () => {
    const summary = applyFailureSummary([
      { file: "src/new.ts", message: "Refusing to overwrite existing file: D:\\work\\a\\src\\new.ts" },
    ]);
    expect(summary).toContain("src/new.ts");
    expect(summary).toContain("Refusing to overwrite existing file");
  });

  it("多条一行一条，超出的只报个数", () => {
    const summary = applyFailureSummary(
      ["a", "b", "c", "d", "e"].map((name) => ({ file: `${name}.ts`, message: `boom ${name}` }))
    );
    expect(summary?.split("\n")).toHaveLength(4);
    expect(summary).toContain("boom a");
    expect(summary).toContain("(+2)");
  });

  it("没有失败就没有横幅", () => {
    expect(applyFailureSummary([])).toBeNull();
  });
});

