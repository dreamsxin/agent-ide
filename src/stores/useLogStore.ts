import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { LogEntry } from "../types/project";
import { isTauriRuntime } from "../utils/tauri";

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
      appendToDiskLog(entry);
      return { logs };
    }),

  clearLogs: () => {
    persistLogs([]);
    set({ logs: [] });
  },

  restoreLogs: (workspacePath) => set({ logs: loadLogs(workspacePath) }),
}));

/**
 * 顺手把同一条记录写进磁盘日志。
 *
 * 面板里这份只在内存 + localStorage 里，换台机器、关掉窗口就没了；而后端事件早就
 * 在落盘了。终端和任务运行的记录只有前端知道，缺了它们，磁盘上那份日志在排查
 * "命令为什么失败"时正好少了需要的那半边。
 *
 * 不 await、失败不上报：这是记录，不是用户要的那个操作。格式和 2 MB 轮转都由后端
 * `run_log` 决定 —— 前端自己拼一份就会和 Agent 那半边的格式慢慢分叉。
 */
function appendToDiskLog(entry: Omit<LogEntry, "id">) {
  if (!isTauriRuntime()) return;
  void invoke("append_ui_log", { entry }).catch(() => {});
}

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
