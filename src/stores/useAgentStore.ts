import { create } from "zustand";
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

  // Actions
  setState: (state: AgentState) => void;
  setMode: (mode: AgentMode) => void;
  setCurrentTask: (task: Task | null) => void;
  addTask: (task: Task) => void;
  updateStep: (stepId: string, status: Step["status"]) => void;
  addDiff: (diff: DiffEntry) => void;
  applyDiff: (diffId: string) => void;
  rejectDiff: (diffId: string) => void;
  setError: (error: string | null) => void;
  reset: () => void;
}

export const useAgentStore = create<AgentStore>((set) => ({
  state: "idle",
  mode: "suggest",
  currentTask: null,
  tasks: [],
  diffs: [],
  steps: [],
  error: null,

  setState: (state) => set({ state }),
  setMode: (mode) => set({ mode }),
  setCurrentTask: (currentTask) => set({ currentTask }),
  addTask: (task) =>
    set((s) => ({ tasks: [...s.tasks, task], currentTask: task })),
  updateStep: (stepId, status) =>
    set((s) => ({
      steps: s.steps.map((st) =>
        st.id === stepId ? { ...st, status } : st
      ),
    })),
  addDiff: (diff) => set((s) => ({ diffs: [...s.diffs, diff] })),
  applyDiff: (diffId) =>
    set((s) => ({
      diffs: s.diffs.map((d) =>
        d.id === diffId ? { ...d, status: "applied" as const } : d
      ),
    })),
  rejectDiff: (diffId) =>
    set((s) => ({
      diffs: s.diffs.map((d) =>
        d.id === diffId ? { ...d, status: "rejected" as const } : d
      ),
    })),
  setError: (error) => set({ error, state: error ? "error" : "idle" }),
  reset: () =>
    set({
      state: "idle",
      currentTask: null,
      steps: [],
      diffs: [],
      error: null,
    }),
}));
