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
  ContextEstimateResponse,
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
  agentRunId: string | null;
  restoredSession: AgentRestoredSession | null;

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
  restoreDiffs: (workspacePath?: string) => void;
  restoreAgentSession: (workspacePath?: string) => void;
  clearAgentSession: () => void;
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
    contextSources?: {
      includeProjectTree?: boolean;
      includeGitDiff?: boolean;
    };
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
  estimateContext: (params: AgentContextParams) => Promise<ContextEstimateResponse | null>;
  updateAgentStep: (step: Step) => Promise<Step | null>;
  skipAgentStep: (stepId: string) => Promise<Step | null>;
  runAgentStep: (params: AgentStepRunParams) => Promise<void>;
  regenerateDiff: (params: RegenerateDiffParams) => Promise<void>;

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

interface AgentContextParams {
  contextFiles?: string[];
  activeFile?: string;
  activeFileContent?: string;
  selection?: string;
  profileId?: string;
  contextCompression?: ContextCompressionMode;
  contextSources?: {
    includeProjectTree?: boolean;
    includeGitDiff?: boolean;
  };
}

interface AgentStepRunParams extends AgentContextParams {
  step: Step;
  extraPrompt?: string;
  regeneratedFromDiffId?: string;
  regeneratedFromHunkIndex?: number;
}

interface RegenerateDiffParams extends AgentContextParams {
  diff: DiffEntry;
  hunkIndex?: number;
  currentFileContent?: string;
}

interface AgentRestoredSession {
  runId: string | null;
  restoredAt: number;
  interrupted: boolean;
  updatedAt?: number;
}

const DEFAULT_PIPELINE: PipelineStage[] = [
  { role: "architect", name: "Design", status: "pending" },
  { role: "coder", name: "Implement", status: "pending" },
  { role: "tester", name: "Test", status: "pending" },
  { role: "reviewer", name: "Review", status: "pending" },
];

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
  agentRunId: null,
  restoredSession: null,
  messages: [
    {
      id: "welcome",
      role: "system" as const,
      content: "Welcome to Agent IDE. I'm your AI coding assistant. Try selecting code for quick actions, or ask me to build something.",
      timestamp: Date.now(),
    },
  ],
  activeRole: "coder",
  pipeline: DEFAULT_PIPELINE,
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
  setState: (state) => {
    set({ state });
    persistAgentSession(get());
  },
  setMode: (mode) => {
    set({ mode });
    persistAgentSession(get());
  },
  setCurrentTask: (currentTask) => {
    set({ currentTask });
    persistAgentSession(get());
  },
  addTask: (task) =>
    set((s) => {
      const next = { tasks: [...s.tasks, task], currentTask: task };
      windowQueuePersist(() => persistAgentSession({ ...get(), ...next }));
      return next;
    }),
  setSteps: (steps) => {
    set({ steps });
    persistAgentSession(get());
  },
  setPipeline: (pipeline) => {
    set({ pipeline });
    persistAgentSession(get());
  },
  updateStep: (stepId, updates) =>
    set((s) => {
      const nextSteps = s.steps.map((st) =>
        st.id === stepId ? { ...st, ...updates } : st
      );
      windowQueuePersist(() => persistAgentSession({ ...get(), steps: nextSteps }));
      return { steps: nextSteps };
    }),
  setDiffs: (diffs) => {
    persistDiffs(diffs);
    set({ diffs, lastApplyResult: null });
  },
  restoreDiffs: (workspacePath) => set({ diffs: loadDiffs(workspacePath), lastApplyResult: null }),
  restoreAgentSession: (workspacePath) => {
    const restored = loadAgentSession(workspacePath);
    if (!restored) return;
    set(restored);
  },
  clearAgentSession: () => {
    clearPersistedAgentSession();
    set({
      state: "idle",
      currentTask: null,
      tasks: [],
      steps: [],
      pipeline: DEFAULT_PIPELINE,
      error: null,
      streamContent: "",
      isStreaming: false,
      agentRunId: null,
      restoredSession: null,
    });
  },
  addDiff: (diff) =>
    set((s) => {
      const diffs = [...s.diffs, diff];
      persistDiffs(diffs);
      return { diffs };
    }),
  markDiffApplied: (diffId) =>
    set((s) => {
      const diffs = s.diffs.map((d) =>
        d.id === diffId ? { ...d, status: "applied" as const, applyError: undefined } : d
      );
      persistDiffs(diffs);
      return { diffs };
    }),
  markDiffRejected: (diffId) =>
    set((s) => {
      const diffs = s.diffs.map((d) =>
        d.id === diffId ? { ...d, status: "rejected" as const, applyError: undefined } : d
      );
      persistDiffs(diffs);
      return { diffs };
    }),
  setError: (error) => {
    set({ error, state: error ? "error" : "idle" });
    persistAgentSession(get());
  },
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
  reset: () => {
    clearPersistedAgentSession();
    set({
      state: "idle",
      currentTask: null,
      steps: [],
      diffs: [],
      error: null,
      lastApplyResult: null,
      streamContent: "",
      isStreaming: false,
      agentRunId: null,
      restoredSession: null,
    });
  },

  // ========== 异步 Actions (IPC) ==========
  sendPrompt: async (params) => {
    set({
      error: null,
      lastApplyResult: null,
      streamContent: "",
      isStreaming: true,
      agentRunId: makeAgentRunId("chat"),
      restoredSession: null,
    });
    persistAgentSession(get());
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
          contextSources: params.contextSources ?? null,
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
        set({ state: "idle", steps: [], diffs: [], agentRunId: null, restoredSession: null });
        persistAgentSession(get());
        return;
      }
      await invoke("stop_agent");
      set({ state: "idle", steps: [], diffs: [], agentRunId: null, restoredSession: null });
      persistAgentSession(get());
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
      set((s) => {
        const diffs = s.diffs.map((d) => {
          if (result.applied.some((a) => a.id === d.id)) {
            return { ...d, status: "applied" as const, applyError: undefined };
          }
          const failure = result.failed.find((f) => f.diffId === d.id);
          if (failure) {
            return { ...d, status: "failed" as const, applyError: failure.message };
          }
          return d;
        });
        persistDiffs(diffs);
        return {
          lastApplyResult: result,
          error: result.failed.length > 0
            ? `Failed to apply ${result.failed.length} diff${result.failed.length === 1 ? "" : "s"}.`
            : null,
          diffs,
        };
      });
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
      set((s) => {
        const diffs = s.diffs.map((d) => {
          if (result.applied.some((a) => a.id === d.id)) {
            return { ...d, status: "applied" as const, applyError: undefined };
          }
          const failure = result.failed.find((f) => f.diffId === d.id);
          if (failure) {
            return { ...d, status: "failed" as const, applyError: failure.message };
          }
          return d;
        });
        persistDiffs(diffs);
        return {
          lastApplyResult: result,
          error: result.failed.length > 0 ? "Failed to apply diff." : null,
          diffs,
        };
      });
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
      set((s) => {
        const diffs = s.diffs.map((d) => {
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
        });
        persistDiffs(diffs);
        return {
          lastApplyResult: result,
          error: result.failed.length > 0 ? "Failed to apply hunk." : null,
          diffs,
        };
      });
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
      set((s) => {
        const diffs = s.diffs.map((d) =>
          rejected.some((r) => r.id === d.id)
            ? { ...d, status: "rejected" as const, applyError: undefined }
            : d
        );
        persistDiffs(diffs);
        return { lastApplyResult: null, diffs };
      });
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
      set((s) => {
        const diffs = s.diffs.map((d) =>
          d.id === rejected.id
            ? { ...d, status: "rejected" as const, applyError: undefined }
            : d
        );
        persistDiffs(diffs);
        return { lastApplyResult: null, diffs };
      });
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
      set((s) => {
        const diffs = s.diffs.map((d) => (d.id === rejected.id ? rejected : d));
        persistDiffs(diffs);
        return { lastApplyResult: null, diffs };
      });
      return rejected;
    } catch (err: unknown) {
      console.warn("[AgentStore] reject_diff_hunk failed:", err);
      return null;
    }
  },

  estimateContext: async (params) => {
    try {
      if (!isTauriRuntime()) return null;
      return await invoke<ContextEstimateResponse>("estimate_agent_context", {
        request: {
          contextFiles: params.contextFiles ?? [],
          activeFile: params.activeFile ?? null,
          activeFileContent: params.activeFileContent ?? null,
          selection: params.selection ?? null,
          profileId: params.profileId ?? get().chatProfileId,
          contextCompression: params.contextCompression ?? get().chatContextCompression,
          contextSources: params.contextSources ?? null,
        },
      });
    } catch (err: unknown) {
      console.warn("[AgentStore] estimate_agent_context failed:", err);
      return null;
    }
  },

  updateAgentStep: async (step) => {
    try {
      if (!isTauriRuntime()) {
        get().updateStep(step.id, step);
        return step;
      }
      const updated = await invoke<Step>("update_agent_step", { step });
      get().updateStep(updated.id, updated);
      return updated;
    } catch (err: unknown) {
      console.warn("[AgentStore] update_agent_step failed:", err);
      return null;
    }
  },

  skipAgentStep: async (stepId) => {
    try {
      if (!isTauriRuntime()) {
        get().updateStep(stepId, { status: "skipped" });
        return get().steps.find((step) => step.id === stepId) ?? null;
      }
      const updated = await invoke<Step>("skip_agent_step", { stepId });
      get().updateStep(updated.id, updated);
      return updated;
    } catch (err: unknown) {
      console.warn("[AgentStore] skip_agent_step failed:", err);
      return null;
    }
  },

  runAgentStep: async (params) => {
    set({
      error: null,
      lastApplyResult: null,
      streamContent: "",
      isStreaming: true,
      agentRunId: makeAgentRunId("step"),
      restoredSession: null,
    });
    persistAgentSession(get());
    try {
      if (!isTauriRuntime()) {
        throw new Error("Agent backend is available in the Tauri app runtime.");
      }
      await invoke("run_agent_step", {
        request: {
          step: params.step,
          contextFiles: params.contextFiles ?? [],
          activeFile: params.activeFile ?? null,
          activeFileContent: params.activeFileContent ?? null,
          selection: params.selection ?? null,
          profileId: params.profileId ?? get().chatProfileId,
          contextCompression: params.contextCompression ?? get().chatContextCompression,
          contextSources: params.contextSources ?? null,
          extraPrompt: params.extraPrompt ?? null,
          regeneratedFromDiffId: params.regeneratedFromDiffId ?? null,
          regeneratedFromHunkIndex: params.regeneratedFromHunkIndex ?? null,
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

  regenerateDiff: async (params) => {
    const hunk = params.hunkIndex != null ? params.diff.hunks[params.hunkIndex] : undefined;
    const targetHunks = hunk ? [hunk] : params.diff.hunks;
    const prompt = [
      "Regenerate this Agent IDE diff against the current file content.",
      "Keep the original intent, but make the replacement hunks match the file as it exists now.",
      "Return reviewable Agent IDE diffs for this file only.",
      "",
      `File: ${params.diff.file}`,
      `Failed diff id: ${params.diff.id}`,
      params.hunkIndex != null ? `Failed hunk index: ${params.hunkIndex}` : null,
      params.diff.applyError ? `Apply error: ${params.diff.applyError}` : null,
      params.diff.provenance ? `Original provenance: ${JSON.stringify(params.diff.provenance)}` : null,
      "",
      "Original generated hunks:",
      JSON.stringify(targetHunks, null, 2),
      "",
      "Current file content:",
      "```",
      params.currentFileContent ?? params.activeFileContent ?? "",
      "```",
    ].filter(Boolean).join("\n");

    await get().runAgentStep({
      step: {
        id: `regen-${params.diff.id}-${params.hunkIndex ?? "file"}-${Date.now()}`,
        title: `Regenerate ${params.diff.file}`,
        type: "edit",
        status: "todo",
        logs: [],
        scope: "active_file",
        executionMode: "fix",
      },
      contextFiles: params.contextFiles,
      activeFile: params.activeFile ?? params.diff.file,
      activeFileContent: params.currentFileContent ?? params.activeFileContent,
      selection: params.selection,
      profileId: params.profileId,
      contextCompression: params.contextCompression,
      contextSources: params.contextSources,
      extraPrompt: prompt,
      regeneratedFromDiffId: params.diff.id,
      regeneratedFromHunkIndex: params.hunkIndex,
    });
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

const AGENT_DIFFS_STORAGE_KEY = "agent-ide-agent-diffs";
const AGENT_SESSION_STORAGE_KEY = "agent-ide-agent-session";

interface PersistedAgentSession {
  workspacePath: string;
  runId: string | null;
  state: AgentState;
  mode: AgentMode;
  currentTask: Task | null;
  tasks: Task[];
  steps: Step[];
  pipeline: PipelineStage[];
  error: string | null;
  updatedAt: number;
}

function persistDiffs(diffs: DiffEntry[]) {
  if (typeof window === "undefined") return;
  const workspacePath = currentWorkspacePath();
  const payload = {
    workspacePath,
    diffs: diffs.slice(-200),
  };
  localStorage.setItem(AGENT_DIFFS_STORAGE_KEY, JSON.stringify(payload));
}

function loadDiffs(expectedWorkspacePath = currentWorkspacePath()): DiffEntry[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = localStorage.getItem(AGENT_DIFFS_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as { workspacePath?: string; diffs?: DiffEntry[] };
    if (!Array.isArray(parsed.diffs)) return [];
    if (expectedWorkspacePath && parsed.workspacePath && parsed.workspacePath !== expectedWorkspacePath) {
      return [];
    }
    return parsed.diffs.slice(-200);
  } catch {
    return [];
  }
}

function persistAgentSession(state: Pick<AgentStore, "state" | "mode" | "currentTask" | "tasks" | "steps" | "pipeline" | "error" | "agentRunId">) {
  if (typeof window === "undefined") return;
  const workspacePath = currentWorkspacePath();
  const hasSessionData = state.steps.length > 0 || state.pipeline.some((stage) => stage.status !== "pending") || state.currentTask !== null;
  if (!workspacePath || !hasSessionData) {
    clearPersistedAgentSession();
    return;
  }
  const payload: PersistedAgentSession = {
    workspacePath,
    runId: state.agentRunId,
    state: normalizeRestoredAgentState(state.state),
    mode: state.mode,
    currentTask: state.currentTask,
    tasks: state.tasks.slice(-50),
    steps: state.steps.slice(-100),
    pipeline: state.pipeline,
    error: state.error,
    updatedAt: Date.now(),
  };
  localStorage.setItem(AGENT_SESSION_STORAGE_KEY, JSON.stringify(payload));
}

function loadAgentSession(expectedWorkspacePath = currentWorkspacePath()): Partial<AgentStore> | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = localStorage.getItem(AGENT_SESSION_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<PersistedAgentSession>;
    if (expectedWorkspacePath && parsed.workspacePath && parsed.workspacePath !== expectedWorkspacePath) {
      return null;
    }
    const steps = Array.isArray(parsed.steps) ? parsed.steps : [];
    const pipeline = Array.isArray(parsed.pipeline) && parsed.pipeline.length > 0
      ? parsed.pipeline
      : DEFAULT_PIPELINE;
    if (steps.length === 0 && pipeline.every((stage) => stage.status === "pending") && !parsed.currentTask) {
      return null;
    }
    const interrupted = isInFlightState(parsed.state);
    return {
      state: normalizeRestoredAgentState(parsed.state),
      mode: parsed.mode ?? "suggest",
      currentTask: parsed.currentTask ?? null,
      tasks: Array.isArray(parsed.tasks) ? parsed.tasks : [],
      steps: steps.map(normalizeRestoredStep),
      pipeline: pipeline.map(normalizeRestoredPipelineStage),
      error: parsed.error ?? null,
      agentRunId: parsed.runId ?? null,
      restoredSession: {
        runId: parsed.runId ?? null,
        restoredAt: Date.now(),
        interrupted,
        updatedAt: parsed.updatedAt,
      },
      streamContent: "",
      isStreaming: false,
    };
  } catch {
    return null;
  }
}

function clearPersistedAgentSession() {
  if (typeof window === "undefined") return;
  localStorage.removeItem(AGENT_SESSION_STORAGE_KEY);
}

function normalizeRestoredAgentState(state?: AgentState): AgentState {
  if (state === "thinking" || state === "planning" || state === "acting" || state === "reviewing") {
    return "waiting_user";
  }
  return state ?? "idle";
}

function isInFlightState(state?: AgentState): boolean {
  return state === "thinking" || state === "planning" || state === "acting" || state === "reviewing";
}

function normalizeRestoredStep(step: Step): Step {
  if (step.status === "doing") {
    return {
      ...step,
      status: "error",
      logs: [...(step.logs ?? []), "Interrupted by reload before completion."],
    };
  }
  return { ...step, logs: step.logs ?? [] };
}

function normalizeRestoredPipelineStage(stage: PipelineStage): PipelineStage {
  return stage.status === "active" ? { ...stage, status: "failed" } : stage;
}

function windowQueuePersist(callback: () => void) {
  if (typeof window === "undefined") return;
  window.queueMicrotask(callback);
}

function makeAgentRunId(prefix: string) {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function currentWorkspacePath() {
  try {
    return localStorage.getItem("agent-ide-workspace-path") ?? "";
  } catch {
    return "";
  }
}
