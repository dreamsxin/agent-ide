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
 * 这条记录该不该由前端写进磁盘日志。
 *
 * `source: "agent"` 的条目是 `useAgentBridge` 把后端事件搬进面板的副本，而后端在发事件时
 * **已经**写过磁盘（`impl RunEvents for AppHandle`）。真机日志里因此出现了逐字重复的两段：
 * 一段 `[info] prompt ...`、紧跟一段 `[ui:info] agent ...`，内容一模一样。后果不只是难看
 * —— 文件长一倍，2 MB 轮转提前一半到达，排查时还要先分辨哪两行是同一件事。
 *
 * 判据落在"这条记录是谁产生的"，不是"内容像不像"：内容比较迟早会把两件真的不同的事判成一件。
 */
export function shouldRecordOnDisk(source: LogEntry["source"]): boolean {
  return source !== "agent";
}

/**
 * 顺手把同一条记录写进磁盘日志。
 *
 * 面板里这份只在内存 + localStorage 里，换台机器、关掉窗口就没了；而终端、任务运行、git
 * 的记录只有前端知道，缺了它们，磁盘上那份日志在排查"命令为什么失败"时正好少了需要的那半边。
 *
 * 时间戳这里**自己生成 ISO-8601（UTC）**，不用条目的 `time`：`time` 是给人看的本地时刻
 * （形如 `20:12:16`，没有日期），而后端写的是 RFC3339 UTC。同一个文件里混两种格式、还差
 * 一个时区，就没法按时间排一遍 —— 这是真机日志里看到的第二个缺陷。
 *
 * 不 await、失败不上报：这是记录，不是用户要的那个操作。
 */
function appendToDiskLog(entry: Omit<LogEntry, "id">) {
  if (!isTauriRuntime() || !shouldRecordOnDisk(entry.source)) return;
  void invoke("append_ui_log", {
    entry: { ...entry, time: new Date().toISOString() },
  }).catch(() => {});
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
