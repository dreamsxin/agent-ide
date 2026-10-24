import { describe, expect, it } from "vitest";
import type { LlmConnectionState, LlmProfile } from "../types/agent";
import {
  llmConnectionCheckedAt,
  llmConnectionIndicator,
  llmIndicator,
  llmTargetFingerprint,
  UNVERIFIED_LLM_CONNECTION,
} from "./llmConnection";

function profile(overrides: Partial<LlmProfile> & { id: string }): LlmProfile {
  return {
    name: overrides.id,
    provider: "openai",
    endpoint: "https://api.example.com/v1/chat/completions",
    api_key_masked: "sk-1****cdef",
    model: "gpt-4o",
    ...overrides,
  };
}

function source(overrides: Partial<Parameters<typeof llmTargetFingerprint>[0]> = {}) {
  return {
    llmProfiles: [profile({ id: "p1" })],
    chatProfileId: "p1",
    activeProfileId: "p1",
    llmEndpoint: "",
    llmModel: "",
    apiKeyMasked: "",
    ...overrides,
  };
}

const TARGET = llmTargetFingerprint(source());

const VERIFIED: LlmConnectionState = {
  status: "ok",
  checkedAt: 1_700_000_000_000,
  detail: "pong",
  target: TARGET,
};

describe("llmTargetFingerprint", () => {
  it("指纹取的是 chat profile，而不是 active profile —— 测试打到的就是前者", () => {
    const fingerprint = llmTargetFingerprint(
      source({
        llmProfiles: [profile({ id: "p1" }), profile({ id: "p2", model: "claude-3-5-sonnet" })],
        chatProfileId: "p2",
      })
    );
    expect(fingerprint).toContain("claude-3-5-sonnet");
    expect(fingerprint).not.toContain("gpt-4o");
  });

  it("没选 chat profile 时退回 active profile", () => {
    expect(llmTargetFingerprint(source({ chatProfileId: null }))).toBe(TARGET);
  });

  it("换端点、换模型、换 key 都会得到不同的指纹", () => {
    const movedEndpoint = source({
      llmProfiles: [profile({ id: "p1", endpoint: "http://localhost:11434/v1/chat/completions" })],
    });
    const movedModel = source({ llmProfiles: [profile({ id: "p1", model: "gpt-4o-mini" })] });
    // 过期的 key 就是"配置齐全但打不通"，而端点和模型一个字都没变
    const rotatedKey = source({
      llmProfiles: [profile({ id: "p1", api_key_masked: "sk-9****wxyz" })],
    });
    expect(llmTargetFingerprint(movedEndpoint)).not.toBe(TARGET);
    expect(llmTargetFingerprint(movedModel)).not.toBe(TARGET);
    expect(llmTargetFingerprint(rotatedKey)).not.toBe(TARGET);
  });

  /**
   * 临时换模型也换了目标。
   *
   * 不算进指纹的话，状态栏那个绿点会替一个**从未测过**的模型作保 —— 而这个函数存在的
   * 全部理由就是"上一次的 ok 说的是另一个目标"。
   */
  it("临时换模型之后，上一次的验证结果不再作数", () => {
    const overridden = source({ chatModelOverride: "gpt-4o-mini" });

    expect(llmTargetFingerprint(overridden)).not.toBe(TARGET);
    // 空白等于没换：清空输入框不该让绿点失效
    expect(llmTargetFingerprint(source({ chatModelOverride: "   " }))).toBe(TARGET);
    expect(llmTargetFingerprint(source({ chatModelOverride: null }))).toBe(TARGET);
  });

  it("一个 profile 都匹配不上时用 endpoint/model 兜底，改端点仍然换指纹", () => {
    const base = source({
      llmProfiles: [],
      chatProfileId: null,
      activeProfileId: "",
      llmEndpoint: "https://api.example.com/v1/chat/completions",
      llmModel: "gpt-4o",
    });
    expect(
      llmTargetFingerprint({ ...base, llmEndpoint: "https://elsewhere.example.com/v1" })
    ).not.toBe(llmTargetFingerprint(base));
  });
});

describe("llmIndicator", () => {
  it("没配置就是没配置，和连通性无关", () => {
    expect(llmIndicator(false, VERIFIED, TARGET).tone).toBe("error");
  });

  it("配置了但没测过是中间档，不是绿灯 —— 这正是原来那个假绿点", () => {
    const indicator = llmIndicator(true, UNVERIFIED_LLM_CONNECTION, TARGET);
    expect(indicator.tone).toBe("warn");
    expect(indicator.title).toMatch(/not been reached/);
  });

  it("测通了才是绿灯", () => {
    expect(llmIndicator(true, VERIFIED, TARGET).tone).toBe("ok");
  });

  it("测失败是红灯，失败原因跟着一起给出去", () => {
    const indicator = llmIndicator(
      true,
      { status: "failed", checkedAt: 1, detail: "401 Unauthorized", target: TARGET },
      TARGET
    );
    expect(indicator.tone).toBe("error");
    expect(indicator.title).toContain("401 Unauthorized");
  });

  /**
   * 判断放在渲染处的全部理由：能改目标的路径有五六条，其中一条还隔着 `await`
   * （测试进行中用户换了 profile）。只要目标对不上，这份结果就一句话都不许说。
   */
  it("目标变了之后，上一次的 ok 既不算数，细节也不外露", () => {
    const stale = llmIndicator(true, VERIFIED, "p1|https://elsewhere.example.com/v1|gpt-4o|k");
    expect(stale.tone).toBe("warn");
    expect(stale.title).not.toContain("pong");
  });

  it("目标变了之后，上一次的失败也不再算数", () => {
    const stale = llmIndicator(
      true,
      { status: "failed", checkedAt: 1, detail: "401 Unauthorized", target: "old" },
      TARGET
    );
    expect(stale.tone).toBe("warn");
    expect(stale.title).not.toContain("401");
  });
});

describe("llmConnectionIndicator", () => {
  it("和状态栏共用同一档判断，只是措辞更短", () => {
    const short = llmConnectionIndicator(VERIFIED, TARGET);
    expect(short.tone).toBe(llmIndicator(true, VERIFIED, TARGET).tone);
    expect(short.label).not.toBe(llmIndicator(true, VERIFIED, TARGET).label);
  });
});

describe("llmConnectionCheckedAt", () => {
  it("目标对得上才给时间戳，否则那是另一个目标的测试时间", () => {
    expect(llmConnectionCheckedAt(VERIFIED, TARGET)).toBe(VERIFIED.checkedAt);
    expect(llmConnectionCheckedAt(VERIFIED, "other")).toBeNull();
  });
});
