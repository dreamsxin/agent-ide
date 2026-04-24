import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { AgentState, AgentMode, Task, Step, DiffEntry } from "../types/agent";

interface AgentStore {
  // 状态
  state: AgentState;
  mode: AgentMode;
  currentTask: Task | null;
  tasks: Task[];
  diffs: DiffEntry[];
  steps: Step[];
  error: string | null;
  streamContent: string;
  isStreaming: boolean;

  // 同步 Actions
  setState: (state: AgentState) => void;
  setMode: (mode: AgentMode) => void;
  setCurrentTask: (task: Task | null) => void;
  addTask: (task: Task) => void;
  setSteps: (steps: Step[]) => void;
  updateStep: (stepId: string, updates: Partial<Step>) => void;
  setDiffs: (diffs: DiffEntry[]) => void;
  addDiff: (diff: DiffEntry) => void;
  markDiffApplied: (diffId: string) => void;
  markDiffRejected: (diffId: string) => void;
  setError: (error: string | null) => void;
  appendStreamContent: (token: string) => void;
  clearStreamContent: () => void;
  reset: () => void;

  // 异步 Actions (IPC)
  sendPrompt: (params: {
    prompt: string;
    contextFiles?: string[];
    activeFile?: string;
    activeFileContent?: string;
    selection?: string;
  }) => Promise<void>;
  stopAgent: () => Promise<void>;
  changeMode: (mode: AgentMode) => Promise<void>;
  applyAllDiffs: () => Promise<DiffEntry[]>;
  rejectAllDiffs: () => Promise<DiffEntry[]>;
}

export const useAgentStore = create<AgentStore>((set) => ({
  state: "idle",
  mode: "suggest",
  currentTask: null,
  tasks: [],
  diffs: [],
  steps: [],
  error: null,
  streamContent: "",
  isStreaming: false,

  // ========== 同步 Actions ==========
  setState: (state) => set({ state }),
  setMode: (mode) => set({ mode }),
  setCurrentTask: (currentTask) => set({ currentTask }),
  addTask: (task) =>
    set((s) => ({ tasks: [...s.tasks, task], currentTask: task })),
  setSteps: (steps) => set({ steps }),
  updateStep: (stepId, updates) =>
    set((s) => ({
      steps: s.steps.map((st) =>
        st.id === stepId ? { ...st, ...updates } : st
      ),
    })),
  setDiffs: (diffs) => set({ diffs }),
  addDiff: (diff) => set((s) => ({ diffs: [...s.diffs, diff] })),
  markDiffApplied: (diffId) =>
    set((s) => ({
      diffs: s.diffs.map((d) =>
        d.id === diffId ? { ...d, status: "applied" as const } : d
      ),
    })),
  markDiffRejected: (diffId) =>
    set((s) => ({
      diffs: s.diffs.map((d) =>
        d.id === diffId ? { ...d, status: "rejected" as const } : d
      ),
    })),
  setError: (error) => set({ error, state: error ? "error" : "idle" }),
  appendStreamContent: (token) =>
    set((s) => ({
      streamContent: s.streamContent + token,
      isStreaming: true,
    })),
  clearStreamContent: () => set({ streamContent: "", isStreaming: false }),
  reset: () =>
    set({
      state: "idle",
      currentTask: null,
      steps: [],
      diffs: [],
      error: null,
      streamContent: "",
      isStreaming: false,
    }),

  // ========== 异步 Actions (IPC) ==========
  sendPrompt: async (params) => {
    set({ error: null, streamContent: "", isStreaming: true });
    try {
      await invoke("send_agent_prompt", {
        request: {
          prompt: params.prompt,
          contextFiles: params.contextFiles ?? [],
          activeFile: params.activeFile ?? null,
          activeFileContent: params.activeFileContent ?? null,
          selection: params.selection ?? null,
        },
      });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      set({ error: msg, state: "error" });
    } finally {
      set({ isStreaming: false });
    }
  },

  stopAgent: async () => {
    try {
      await invoke("stop_agent");
      set({ state: "idle", steps: [], diffs: [] });
    } catch (err: unknown) {
      console.warn("[AgentStore] stop_agent failed:", err);
    }
  },

  changeMode: async (mode) => {
    try {
      await invoke("set_agent_mode", { mode });
      set({ mode });
    } catch (err: unknown) {
      console.warn("[AgentStore] set_agent_mode failed:", err);
      // 仍然本地更新
      set({ mode });
    }
  },

  applyAllDiffs: async () => {
    try {
      const applied = await invoke<DiffEntry[]>("apply_diffs");
      set((s) => ({
        diffs: s.diffs.map((d) =>
          applied.some((a) => a.id === d.id)
            ? { ...d, status: "applied" as const }
            : d
        ),
      }));
      return applied;
    } catch (err: unknown) {
      console.warn("[AgentStore] apply_diffs failed:", err);
      return [];
    }
  },

  rejectAllDiffs: async () => {
    try {
      const rejected = await invoke<DiffEntry[]>("reject_diffs");
      set((s) => ({
        diffs: s.diffs.map((d) =>
          rejected.some((r) => r.id === d.id)
            ? { ...d, status: "rejected" as const }
            : d
        ),
      }));
      return rejected;
    } catch (err: unknown) {
      console.warn("[AgentStore] reject_diffs failed:", err);
      return [];
    }
  },
}));
