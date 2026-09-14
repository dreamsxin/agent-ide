import type { LlmConnectionState, LlmProfile } from "../types/agent";

/** 没验证过的那一档；也是 store 的初始值 */
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
  apiKeyMasked: string;
}

/**
 * 连通性测试实际打到的那个目标的指纹。
 *
 * 存指纹而不是一个"测过了"的布尔：测的是 `test_llm_connection(chatProfileId)`，
 * 换 profile、改端点、改模型、换 key 之后，上一次的 ok 说的是另一个目标。
 *
 * key 也在指纹里（只有掩码，够用）：换 key 恰恰是最需要重测的时刻 —— 过期的 key
 * 是"配置齐全但打不通"的典型，而端点和模型名一个字都没变。
 */
export function llmTargetFingerprint(source: LlmTargetSource): string {
  const id = source.chatProfileId ?? source.activeProfileId;
  const profile = source.llmProfiles.find((item) => item.id === id);
  if (profile) {
    return `${profile.id}|${profile.endpoint}|${profile.model}|${profile.api_key_masked}`;
  }
  // 一个 profile 都没匹配上时（列表为空、或者刚被删掉）仍然要有指纹，否则端点改了
  // 指纹不变，绿点会替新端点作保。
  return `|${source.llmEndpoint}|${source.llmModel}|${source.apiKeyMasked}`;
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

/**
 * 这份结果还能不能替**当前**目标说话。
 *
 * 在渲染处比，而不是在每个改配置的地方回写 `unknown`：能改目标的路径有五六条，还
 * 有一条隔着 `await`（测试进行中用户换了 profile，结果落回来时目标已经不是它了）。
 * 靠"每一处都记得作废"来维持诚实，漏一处就是一个替新端点作保的绿点；比一次就不会漏。
 */
function effectiveStatus(
  connection: LlmConnectionState,
  currentTarget: string
): LlmConnectionState["status"] {
  return connection.target === currentTarget ? connection.status : "unknown";
}

/** 设置面板里 Connection 那一行 */
export function llmConnectionIndicator(
  connection: LlmConnectionState,
  currentTarget: string
): LlmIndicator {
  const status = effectiveStatus(connection, currentTarget);
  const wording = CONNECTION_WORDING[status];
  // detail 是能拿去排查的东西（失败原因、或者模型回的那句话），所以进 title —— 但只
  // 在它说的还是当前目标时，否则那是另一个端点的错误信息。
  const detail = status === connection.status ? connection.detail : null;
  return {
    label: wording.short,
    tone: wording.tone,
    title: detail ? `${wording.title}: ${detail}` : wording.title,
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
  connection: LlmConnectionState,
  currentTarget: string
): LlmIndicator {
  if (!configured) {
    return {
      label: "LLM not configured",
      tone: "error",
      title: "No API credentials — open the Agent panel's Settings view to add a profile",
    };
  }
  const base = llmConnectionIndicator(connection, currentTarget);
  return {
    ...base,
    label: CONNECTION_WORDING[effectiveStatus(connection, currentTarget)].statusBar,
  };
}

/** 时间戳只在这份结果还算数时有意义 */
export function llmConnectionCheckedAt(
  connection: LlmConnectionState,
  currentTarget: string
): number | null {
  return connection.target === currentTarget ? connection.checkedAt : null;
}
