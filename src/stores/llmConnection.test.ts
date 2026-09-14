import { describe, expect, it } from "vitest";
import type { LlmConnectionState, LlmProfile } from "../types/agent";
import {
  connectionForTarget,
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

const VERIFIED: LlmConnectionState = {
  status: "ok",
  checkedAt: 1_700_000_000_000,
  detail: "pong",
  target: "p1|https://api.example.com/v1/chat/completions|gpt-4o",
};

describe("llmTargetFingerprint", () => {
  it("指纹取的是 chat profile，而不是 active profile —— 测试打到的就是前者", () => {
    const source = {
      llmProfiles: [profile({ id: "p1" }), profile({ id: "p2", model: "claude-3-5-sonnet" })],
      chatProfileId: "p2",
      activeProfileId: "p1",
      llmEndpoint: "",
      llmModel: "",
    };
    expect(llmTargetFingerprint(source)).toContain("claude-3-5-sonnet");
    expect(llmTargetFingerprint(source)).not.toContain("gpt-4o");
  });

  it("没选 chat profile 时退回 active profile", () => {
    const source = {
      llmProfiles: [profile({ id: "p1" })],
      chatProfileId: null,
      activeProfileId: "p1",
      llmEndpoint: "",
      llmModel: "",
    };
    expect(llmTargetFingerprint(source)).toBe(
      "p1|https://api.example.com/v1/chat/completions|gpt-4o"
    );
  });

  it("换端点或换模型都会得到不同的指纹", () => {
    const base = {
      llmProfiles: [profile({ id: "p1" })],
      chatProfileId: "p1",
      activeProfileId: "p1",
      llmEndpoint: "",
      llmModel: "",
    };
    const movedEndpoint = {
      ...base,
      llmProfiles: [profile({ id: "p1", endpoint: "http://localhost:11434/v1/chat/completions" })],
    };
    const movedModel = { ...base, llmProfiles: [profile({ id: "p1", model: "gpt-4o-mini" })] };
    expect(llmTargetFingerprint(movedEndpoint)).not.toBe(llmTargetFingerprint(base));
    expect(llmTargetFingerprint(movedModel)).not.toBe(llmTargetFingerprint(base));
  });

  it("profile 列表里找不到目标时用 endpoint/model 兜底，改端点仍然换指纹", () => {
    const base = {
      llmProfiles: [],
      chatProfileId: null,
      activeProfileId: "",
      llmEndpoint: "https://api.example.com/v1/chat/completions",
      llmModel: "gpt-4o",
    };
    expect(
      llmTargetFingerprint({ ...base, llmEndpoint: "https://elsewhere.example.com/v1" })
    ).not.toBe(llmTargetFingerprint(base));
  });
});

describe("connectionForTarget", () => {
  it("目标没变就保留已验证的结果", () => {
    expect(connectionForTarget(VERIFIED, VERIFIED.target as string)).toBe(VERIFIED);
  });

  it("目标一变就作废，不让旧的 ok 替新端点作保", () => {
    const next = connectionForTarget(VERIFIED, "p1|https://elsewhere.example.com/v1|gpt-4o");
    expect(next.status).toBe("unknown");
    expect(next.detail).toBeNull();
    expect(next.target).toBeNull();
  });
});

describe("llmIndicator", () => {
  it("没配置就是没配置，和连通性无关", () => {
    expect(llmIndicator(false, VERIFIED).tone).toBe("error");
  });

  it("配置了但没测过是中间档，不是绿灯 —— 这正是原来那个假绿点", () => {
    const indicator = llmIndicator(true, UNVERIFIED_LLM_CONNECTION);
    expect(indicator.tone).toBe("warn");
    expect(indicator.title).toMatch(/not been reached/);
  });

  it("测通了才是绿灯", () => {
    expect(llmIndicator(true, VERIFIED).tone).toBe("ok");
  });

  it("测失败是红灯，失败原因跟着一起给出去", () => {
    const indicator = llmIndicator(true, {
      status: "failed",
      checkedAt: 1,
      detail: "401 Unauthorized",
      target: "t",
    });
    expect(indicator.tone).toBe("error");
    expect(indicator.title).toContain("401 Unauthorized");
  });
});

describe("llmConnectionIndicator", () => {
  it("和状态栏共用同一档判断，只是措辞更短", () => {
    const short = llmConnectionIndicator(VERIFIED);
    expect(short.tone).toBe(llmIndicator(true, VERIFIED).tone);
    expect(short.label).not.toBe(llmIndicator(true, VERIFIED).label);
  });
});
