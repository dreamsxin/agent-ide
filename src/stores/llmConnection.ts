import type { LlmConnectionState, LlmProfile } from "../types/agent";

/** 没验证过的那一档，也是目标变化后要回到的那一档 */
export const UNVERIFIED_LLM_CONNECTION: LlmConnectionState = {
  status: "unknown",
  checkedAt: null,
  detail: null,
  target: null,
};

/** `llmTargetFingerprint` 需要的那几个字段，避免让纯函数依赖整个 store 类型 */
export interface LlmTargetSource {
  llmProfiles: LlmProfile[];
  chatProfileId: string | null;
  activeProfileId: string;
  llmEndpoint: string;
  llmModel: string;
}

/**
 * 连通性测试实际打到的那个目标的指纹。
 *
 * 存指纹而不是一个"测过了"的布尔：测的是 `test_llm_connection(chatProfileId)`，
 * 换 profile、改端点、改模型之后，上一次的 ok 说的是另一个目标。指纹一对不上就
 * 不能再拿它当证据。
 */
export function llmTargetFingerprint(source: LlmTargetSource): string {
  const id = source.chatProfileId ?? source.activeProfileId;
  const profile = source.llmProfiles.find((item) => item.id === id);
  if (profile) {
    return `${profile.id}|${profile.endpoint}|${profile.model}`;
  }
  // profile 列表里找不到时（`update_llm_config` 那条旧的单配置路径）仍然要有指纹，
  // 否则端点改了指纹不变，绿点会替新端点作保。
  return `|${source.llmEndpoint}|${source.llmModel}`;
}

/** 目标没变就保留上次结果，变了就作废 */
export function connectionForTarget(
  previous: LlmConnectionState,
  target: string
): LlmConnectionState {
  return previous.target === target ? previous : UNVERIFIED_LLM_CONNECTION;
}

export interface LlmIndicator {
  label: string;
  tone: "ok" | "warn" | "error";
  title: string;
}

/**
 * 三档连通性各自的措辞。
 *
 * 一处定义、两处措辞：状态栏要一句自带主语的话（"LLM connected"），设置面板里那张
 * 卡的 Connection 一行只需要值（"Verified"）。分开写成两个映射的话，改了一处忘了
 * 另一处，同一个状态在两个地方就会讲不同的故事。
 */
const CONNECTION_WORDING = {
  ok: {
    tone: "ok",
    statusBar: "LLM connected",
    short: "Verified",
    title: "Connection verified",
  },
  failed: {
    tone: "error",
    statusBar: "LLM unreachable",
    short: "Failed",
    title: "Last connection test failed",
  },
  unknown: {
    tone: "warn",
    statusBar: "LLM configured",
    short: "Not tested",
    title:
      "A profile is configured but the endpoint has not been reached — click Test in the Agent panel's Settings view",
  },
} as const satisfies Record<
  LlmConnectionState["status"],
  { tone: LlmIndicator["tone"]; statusBar: string; short: string; title: string }
>;

/** 设置面板里 Connection 那一行 */
export function llmConnectionIndicator(connection: LlmConnectionState): LlmIndicator {
  const wording = CONNECTION_WORDING[connection.status];
  return {
    label: wording.short,
    tone: wording.tone,
    // detail 才是能拿去排查的东西（失败原因、或者模型回的那句话），所以进 title
    title: connection.detail ? `${wording.title}: ${connection.detail}` : wording.title,
  };
}

/**
 * 状态栏那一句。
 *
 * 三档而不是两档：以前只有 `llmConfigured`，也就是"存过 profile"就亮绿点说
 * "LLM ready" —— 端点打不通它并不知道，而那正是用户点了发送才发现的那种失败。
 */
export function llmIndicator(
  configured: boolean,
  connection: LlmConnectionState
): LlmIndicator {
  if (!configured) {
    return {
      label: "LLM not configured",
      tone: "error",
      title: "No API credentials — open the Agent panel's Settings view to add a profile",
    };
  }
  const base = llmConnectionIndicator(connection);
  return { ...base, label: CONNECTION_WORDING[connection.status].statusBar };
}

