import { create } from "zustand";
import type { LogEntry } from "../types/project";

let _nextId = 1;
const LOG_STORAGE_KEY = "agent-ide-logs";
const MAX_LOGS = 500;

interface LogStore {
  logs: LogEntry[];

  addLog: (entry: Omit<LogEntry, "id">) => void;
  clearLogs: () => void;
  restoreLogs: (workspacePath?: string) => void;
}

export const useLogStore = create<LogStore>((set) => ({
  logs: loadLogs(),

  addLog: (entry) =>
    set((s) => {
      const logs = [
        ...s.logs,
        { ...entry, id: String(_nextId++) },
      ].slice(-MAX_LOGS);
      persistLogs(logs);
      return { logs };
    }),

  clearLogs: () => {
    persistLogs([]);
    set({ logs: [] });
  },

  restoreLogs: (workspacePath) => set({ logs: loadLogs(workspacePath) }),
}));

function persistLogs(logs: LogEntry[]) {
  if (typeof window === "undefined") return;
  const workspacePath = currentWorkspacePath();
  const payload = {
    workspacePath,
    logs,
  };
  localStorage.setItem(LOG_STORAGE_KEY, JSON.stringify(payload));
}

function loadLogs(expectedWorkspacePath = currentWorkspacePath()): LogEntry[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = localStorage.getItem(LOG_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as { workspacePath?: string; logs?: LogEntry[] };
    if (!Array.isArray(parsed.logs)) return [];
    if (expectedWorkspacePath && parsed.workspacePath && parsed.workspacePath !== expectedWorkspacePath) {
      return [];
    }
    const maxId = parsed.logs
      .map((log) => Number(log.id))
      .filter(Number.isFinite)
      .reduce((max, id) => Math.max(max, id), 0);
    _nextId = Math.max(_nextId, maxId + 1);
    return parsed.logs.slice(-MAX_LOGS);
  } catch {
    return [];
  }
}

function currentWorkspacePath() {
  try {
    return localStorage.getItem("agent-ide-workspace-path") ?? "";
  } catch {
    return "";
  }
}
