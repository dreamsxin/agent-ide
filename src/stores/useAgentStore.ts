import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "../utils/tauri";
import type {
  AgentState,
  AgentMode,
  AgentRole,
  ContextCompressionMode,
  PipelineStage,
  LlmConfigResponse,
  LlmProfile,
  LlmProfilesResponse,
  SaveLlmProfileRequest,
  Task,
  Step,
  DiffEntry,
  ApplyDiffsResult,
  ChatMessage,
} from "../types/agent";

interface AgentStore {
  // ====== Agent 状态 ======
  state: AgentState;
  mode: AgentMode;
  currentTask: Task | null;
  tasks: Task[];
  diffs: DiffEntry[];
  steps: Step[];
  error: string | null;
  lastApplyResult: ApplyDiffsResult | null;
  streamContent: string;
  isStreaming: boolean;

  // ====== Chat 消息 ======
  messages: ChatMessage[];

  // ====== 角色与流水线 ======
  activeRole: AgentRole;
  pipeline: PipelineStage[];

  // ====== LLM 配置 ======
  llmConfigured: boolean;
  llmEndpoint: string;
  llmModel: string;
  apiKeyMasked: string;
  contextCompression: ContextCompressionMode;
  llmProfiles: LlmProfile[];
  activeProfileId: string;
  chatProfileId: string | null;
  chatContextCompression: ContextCompressionMode | null;

  // ====== 同步 Actions ======
  setState: (state: AgentState) => void;
  setMode: (mode: AgentMode) => void;
  setCurrentTask: (task: Task | null) => void;
  addTask: (task: Task) => void;
  setSteps: (steps: Step[]) => void;
  updateStep: (stepId: string, updates: Partial<Step>) => void;
  setDiffs: (diffs: DiffEntry[]) => void;
  setPipeline: (stages: PipelineStage[]) => void;
  addDiff: (diff: DiffEntry) => void;
  markDiffApplied: (diffId: string) => void;
  markDiffRejected: (diffId: string) => void;
  setError: (error: string | null) => void;
  appendStreamContent: (token: string) => void;
  clearStreamContent: () => void;
  addMessage: (msg: ChatMessage) => void;
  updateMessage: (id: string, updates: Partial<ChatMessage>) => void;
  clearMessages: () => void;
  reset: () => void;

  // ====== 异步 Actions (IPC) ======
  sendPrompt: (params: {
    prompt: string;
    contextFiles?: string[];
    activeFile?: string;
    activeFileContent?: string;
    selection?: string;
    profileId?: string;
    contextCompression?: ContextCompressionMode;
  }) => Promise<void>;
  stopAgent: () => Promise<void>;
  changeMode: (mode: AgentMode) => Promise<void>;
  applyAllDiffs: () => Promise<DiffEntry[]>;
  applyDiff: (diffId: string) => Promise<DiffEntry[]>;
  applyDiffHunk: (diffId: string, hunkIndex: number) => Promise<DiffEntry[]>;
  clearApplyResult: () => void;
  rejectAllDiffs: () => Promise<DiffEntry[]>;
  rejectDiff: (diffId: string) => Promise<DiffEntry | null>;
  rejectDiffHunk: (diffId: string, hunkIndex: number) => Promise<DiffEntry | null>;

  // ====== 模型配置 ======
  fetchLlmConfig: () => Promise<void>;
  updateLlmConfig: (endpoint: string, apiKey: string, model: string) => Promise<void>;
  saveLlmProfile: (request: SaveLlmProfileRequest) => Promise<void>;
  deleteLlmProfile: (profileId: string) => Promise<void>;
  setActiveLlmProfile: (profileId: string) => Promise<void>;
  setChatProfileId: (profileId: string | null) => void;
  setChatContextCompression: (mode: ContextCompressionMode | null) => void;
  updateContextCompression: (mode: ContextCompressionMode) => Promise<void>;

  // ====== 角色管理 ======
  setActiveRole: (role: AgentRole) => Promise<void>;
  fetchActiveRole: () => Promise<void>;

  // ====== 流水线管理 ======
  fetchPipeline: () => Promise<void>;
  updatePipeline: (stages: PipelineStage[]) => Promise<void>;
  resetPipeline: () => Promise<void>;

  // ====== 连通性测试 ======
  testLlmConnection: () => Promise<string>;
}

export const useAgentStore = create<AgentStore>((set, get) => ({
  // ========== 初始值 ==========
  state: "idle",
  mode: "suggest",
  currentTask: null,
  tasks: [],
  diffs: [],
  steps: [],
  error: null,
  lastApplyResult: null,
  streamContent: "",
  isStreaming: false,
  messages: [
    {
      id: "welcome",
      role: "system" as const,
      content: "Welcome to Agent IDE. I'm your AI coding assistant. Try selecting code for quick actions, or ask me to build something.",
      timestamp: Date.now(),
    },
  ],
  activeRole: "coder",
  pipeline: [
    { role: "architect", name: "Design", status: "pending" },
    { role: "coder", name: "Implement", status: "pending" },
    { role: "tester", name: "Test", status: "pending" },
    { role: "reviewer", name: "Review", status: "pending" },
  ],
  llmConfigured: false,
  llmEndpoint: "",
  llmModel: "",
  apiKeyMasked: "",
  contextCompression: "focused",
  llmProfiles: [],
  activeProfileId: "",
  chatProfileId: null,
  chatContextCompression: null,

  // ========== 同步 Actions ==========
  setState: (state) => set({ state }),
  setMode: (mode) => set({ mode }),
  setCurrentTask: (currentTask) => set({ currentTask }),
  addTask: (task) =>
    set((s) => ({ tasks: [...s.tasks, task], currentTask: task })),
  setSteps: (steps) => set({ steps }),
  setPipeline: (pipeline) => set({ pipeline }),
  updateStep: (stepId, updates) =>
    set((s) => ({
      steps: s.steps.map((st) =>
        st.id === stepId ? { ...st, ...updates } : st
      ),
    })),
  setDiffs: (diffs) => set({ diffs, lastApplyResult: null }),
  addDiff: (diff) => set((s) => ({ diffs: [...s.diffs, diff] })),
  markDiffApplied: (diffId) =>
    set((s) => ({
      diffs: s.diffs.map((d) =>
        d.id === diffId ? { ...d, status: "applied" as const, applyError: undefined } : d
      ),
    })),
  markDiffRejected: (diffId) =>
    set((s) => ({
      diffs: s.diffs.map((d) =>
        d.id === diffId ? { ...d, status: "rejected" as const, applyError: undefined } : d
      ),
    })),
  setError: (error) => set({ error, state: error ? "error" : "idle" }),
  clearApplyResult: () => set({ lastApplyResult: null }),
  appendStreamContent: (token) =>
    set((s) => ({
      streamContent: s.streamContent + token,
      isStreaming: true,
    })),
  clearStreamContent: () => set({ streamContent: "", isStreaming: false }),
  addMessage: (msg) => set((s) => ({ messages: [...s.messages, msg] })),
  updateMessage: (id, updates) =>
    set((s) => ({
      messages: s.messages.map((m) =>
        m.id === id ? { ...m, ...updates } : m
      ),
    })),
  clearMessages: () =>
    set({
      messages: [
        {
          id: "welcome",
          role: "system",
          content: "Welcome to Agent IDE. I'm your AI coding assistant.",
          timestamp: Date.now(),
        },
      ],
    }),
  reset: () =>
    set({
      state: "idle",
      currentTask: null,
      steps: [],
      diffs: [],
      error: null,
      lastApplyResult: null,
      streamContent: "",
      isStreaming: false,
    }),

  // ========== 异步 Actions (IPC) ==========
  sendPrompt: async (params) => {
    set({ error: null, lastApplyResult: null, streamContent: "", isStreaming: true });
    try {
      if (!isTauriRuntime()) {
        throw new Error("Agent backend is available in the Tauri app runtime.");
      }
      await invoke("send_agent_prompt", {
        request: {
          prompt: params.prompt,
          contextFiles: params.contextFiles ?? [],
          activeFile: params.activeFile ?? null,
          activeFileContent: params.activeFileContent ?? null,
          selection: params.selection ?? null,
          profileId: params.profileId ?? get().chatProfileId,
          contextCompression: params.contextCompression ?? get().chatContextCompression,
        },
      });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg === "Agent task cancelled") {
        set({ error: null, state: "idle" });
      } else {
        set({ error: msg, state: "error" });
      }
    } finally {
      set({ isStreaming: false });
    }
  },

  stopAgent: async () => {
    try {
      if (!isTauriRuntime()) {
        set({ state: "idle", steps: [], diffs: [] });
        return;
      }
      await invoke("stop_agent");
      set({ state: "idle", steps: [], diffs: [] });
    } catch (err: unknown) {
      console.warn("[AgentStore] stop_agent failed:", err);
    }
  },

  changeMode: async (mode) => {
    try {
      if (!isTauriRuntime()) {
        set({ mode });
        return;
      }
      await invoke("set_agent_mode", { mode });
      set({ mode });
    } catch (err: unknown) {
      console.warn("[AgentStore] set_agent_mode failed:", err);
      set({ mode });
    }
  },

  applyAllDiffs: async () => {
    try {
      if (!isTauriRuntime()) return [];
      const result = await invoke<ApplyDiffsResult>("apply_diffs");
      set((s) => ({
        lastApplyResult: result,
        error: result.failed.length > 0
          ? `Failed to apply ${result.failed.length} diff${result.failed.length === 1 ? "" : "s"}.`
          : null,
        diffs: s.diffs.map((d) => {
          if (result.applied.some((a) => a.id === d.id)) {
            return { ...d, status: "applied" as const, applyError: undefined };
          }
          const failure = result.failed.find((f) => f.diffId === d.id);
          if (failure) {
            return { ...d, status: "failed" as const, applyError: failure.message };
          }
          return d;
        }),
      }));
      return result.applied;
    } catch (err: unknown) {
      console.warn("[AgentStore] apply_diffs failed:", err);
      return [];
    }
  },

  applyDiff: async (diffId) => {
    try {
      if (!isTauriRuntime()) return [];
      const result = await invoke<ApplyDiffsResult>("apply_diff", { diffId });
      set((s) => ({
        lastApplyResult: result,
        error: result.failed.length > 0 ? "Failed to apply diff." : null,
        diffs: s.diffs.map((d) => {
          if (result.applied.some((a) => a.id === d.id)) {
            return { ...d, status: "applied" as const, applyError: undefined };
          }
          const failure = result.failed.find((f) => f.diffId === d.id);
          if (failure) {
            return { ...d, status: "failed" as const, applyError: failure.message };
          }
          return d;
        }),
      }));
      return result.applied;
    } catch (err: unknown) {
      console.warn("[AgentStore] apply_diff failed:", err);
      return [];
    }
  },

  applyDiffHunk: async (diffId, hunkIndex) => {
    try {
      if (!isTauriRuntime()) return [];
      const result = await invoke<ApplyDiffsResult>("apply_diff_hunk", { diffId, hunkIndex });
      set((s) => ({
        lastApplyResult: result,
        error: result.failed.length > 0 ? "Failed to apply hunk." : null,
        diffs: s.diffs.map((d) => {
          if (d.id !== diffId) return d;
          const failed = result.failed.find((failure) => failure.diffId === d.id);
          const applied = result.applied.some((item) => item.id === d.id);
          const hunks = d.hunks.map((hunk, index) =>
            index === hunkIndex
              ? {
                  ...hunk,
                  status: applied ? "applied" as const : failed ? "failed" as const : hunk.status,
                  applyError: failed?.message,
                }
              : hunk
          );
          return {
            ...d,
            hunks,
            status: nextDiffStatus(hunks),
            applyError: failed?.message,
          };
        }),
      }));
      return result.applied;
    } catch (err: unknown) {
      console.warn("[AgentStore] apply_diff_hunk failed:", err);
      return [];
    }
  },

  rejectAllDiffs: async () => {
    try {
      if (!isTauriRuntime()) return [];
      const rejected = await invoke<DiffEntry[]>("reject_diffs");
      set((s) => ({
        lastApplyResult: null,
        diffs: s.diffs.map((d) =>
          rejected.some((r) => r.id === d.id)
            ? { ...d, status: "rejected" as const, applyError: undefined }
            : d
        ),
      }));
      return rejected;
    } catch (err: unknown) {
      console.warn("[AgentStore] reject_diffs failed:", err);
      return [];
    }
  },

  rejectDiff: async (diffId) => {
    try {
      if (!isTauriRuntime()) return null;
      const rejected = await invoke<DiffEntry>("reject_diff", { diffId });
      set((s) => ({
        lastApplyResult: null,
        diffs: s.diffs.map((d) =>
          d.id === rejected.id
            ? { ...d, status: "rejected" as const, applyError: undefined }
            : d
        ),
      }));
      return rejected;
    } catch (err: unknown) {
      console.warn("[AgentStore] reject_diff failed:", err);
      return null;
    }
  },

  rejectDiffHunk: async (diffId, hunkIndex) => {
    try {
      if (!isTauriRuntime()) return null;
      const rejected = await invoke<DiffEntry>("reject_diff_hunk", { diffId, hunkIndex });
      set((s) => ({
        lastApplyResult: null,
        diffs: s.diffs.map((d) => (d.id === rejected.id ? rejected : d)),
      }));
      return rejected;
    } catch (err: unknown) {
      console.warn("[AgentStore] reject_diff_hunk failed:", err);
      return null;
    }
  },

  // ========== 模型配置 ==========
  fetchLlmConfig: async () => {
    try {
      if (!isTauriRuntime()) {
        set({ llmConfigured: false });
        return;
      }
      const cfg = await invoke<LlmConfigResponse>("get_llm_config");
      set({
        llmConfigured: true,
        llmEndpoint: cfg.endpoint,
        llmModel: cfg.model,
        apiKeyMasked: cfg.api_key_masked,
        contextCompression: cfg.context_compression,
        llmProfiles: cfg.profiles ?? [],
        activeProfileId: cfg.active_profile_id ?? "",
        chatProfileId: get().chatProfileId ?? cfg.active_profile_id ?? null,
      });
    } catch {
      set({ llmConfigured: false });
    }
  },

  updateLlmConfig: async (endpoint, apiKey, model) => {
    if (!isTauriRuntime()) {
      throw new Error("LLM configuration is available in the Tauri app runtime.");
    }
    await invoke("update_llm_config", {
      endpoint,
      apiKey,
      model,
    });
    set({
      llmConfigured: true,
      llmEndpoint: endpoint,
      llmModel: model,
      apiKeyMasked: apiKey.length > 8
        ? apiKey.slice(0, 4) + "****" + apiKey.slice(-4)
        : "****",
    });
  },

  saveLlmProfile: async (request) => {
    if (!isTauriRuntime()) {
      throw new Error("LLM profile management is available in the Tauri app runtime.");
    }
    const response = await invoke<LlmProfilesResponse>("save_llm_profile", { request });
    applyProfilesResponse(response, set, get);
  },

  deleteLlmProfile: async (profileId) => {
    if (!isTauriRuntime()) {
      throw new Error("LLM profile management is available in the Tauri app runtime.");
    }
    const response = await invoke<LlmProfilesResponse>("delete_llm_profile", { profileId });
    applyProfilesResponse(response, set, get);
  },

  setActiveLlmProfile: async (profileId) => {
    if (!isTauriRuntime()) {
      set({ activeProfileId: profileId, chatProfileId: profileId });
      return;
    }
    const response = await invoke<LlmProfilesResponse>("set_active_llm_profile", { profileId });
    applyProfilesResponse(response, set, get);
  },

  setChatProfileId: (profileId) => set({ chatProfileId: profileId }),
  setChatContextCompression: (mode) => set({ chatContextCompression: mode }),

  updateContextCompression: async (mode) => {
    if (!isTauriRuntime()) {
      set({ contextCompression: mode });
      return;
    }
    const saved = await invoke<ContextCompressionMode>("set_context_compression", { mode });
    set({ contextCompression: saved });
  },

  // ========== 角色管理 ==========
  setActiveRole: async (role) => {
    if (!isTauriRuntime()) {
      set({ activeRole: role });
      return;
    }
    await invoke("set_active_role", { role });
    set({ activeRole: role });
  },

  fetchActiveRole: async () => {
    try {
      if (!isTauriRuntime()) return;
      const role = await invoke<string>("get_active_role");
      set({ activeRole: role as AgentRole });
    } catch {
      // keep default
    }
  },

  // ========== 流水线管理 ==========
  fetchPipeline: async () => {
    try {
      if (!isTauriRuntime()) return;
      const stages = await invoke<PipelineStage[]>("get_pipeline");
      set({ pipeline: stages });
    } catch {
      // keep default
    }
  },

  updatePipeline: async (stages) => {
    if (!isTauriRuntime()) {
      set({ pipeline: stages });
      return;
    }
    await invoke("update_pipeline", { stages });
    set({ pipeline: stages });
  },

  resetPipeline: async () => {
    if (!isTauriRuntime()) {
      return;
    }
    const stages = await invoke<PipelineStage[]>("reset_pipeline");
    set({ pipeline: stages });
  },

  // ========== 连通性测试 ==========
  testLlmConnection: async () => {
    if (!isTauriRuntime()) {
      throw new Error("LLM connection test is available in the Tauri app runtime.");
    }
    const result = await invoke<string>("test_llm_connection", {
      profileId: get().chatProfileId,
    });
    return result;
  },
}));

function applyProfilesResponse(
  response: LlmProfilesResponse,
  set: (partial: Partial<AgentStore>) => void,
  get: () => AgentStore
) {
  const active =
    response.profiles.find((profile) => profile.id === response.active_profile_id) ??
    response.profiles[0];
  set({
    llmConfigured: response.profiles.length > 0,
    llmProfiles: response.profiles,
    activeProfileId: response.active_profile_id,
    chatProfileId: get().chatProfileId ?? response.active_profile_id,
    contextCompression: response.context_compression,
    llmEndpoint: active?.endpoint ?? "",
    llmModel: active?.model ?? "",
    apiKeyMasked: active?.api_key_masked ?? "",
  });
}

function nextDiffStatus(hunks: DiffEntry["hunks"]): DiffEntry["status"] {
  if (hunks.every((hunk) => hunk.status === "applied" || hunk.status === "rejected")) {
    return "applied";
  }
  if (hunks.some((hunk) => hunk.status === "failed")) {
    return "failed";
  }
  return "pending";
}
