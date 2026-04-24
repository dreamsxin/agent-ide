import { create } from "zustand";
import type { LogEntry } from "../types/project";

let _nextId = 1;

interface LogStore {
  logs: LogEntry[];

  addLog: (entry: Omit<LogEntry, "id">) => void;
  clearLogs: () => void;
}

export const useLogStore = create<LogStore>((set) => ({
  logs: [],

  addLog: (entry) =>
    set((s) => ({
      logs: [
        ...s.logs,
        { ...entry, id: String(_nextId++) },
      ].slice(-500), // 最多保留 500 条
    })),

  clearLogs: () => set({ logs: [] }),
}));
