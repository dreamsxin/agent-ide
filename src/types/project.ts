/** 项目信息 */
export interface ProjectInfo {
  name: string;
  path: string;
  language: string;
  git: GitInfo | null;
}

/** Git 信息 */
export interface GitInfo {
  branch: string;
  dirty: boolean;
  ahead: number;
  behind: number;
}

/** Git 状态条目 */
export interface GitStatusEntry {
  path: string;
  status: "modified" | "added" | "deleted" | "untracked" | "renamed";
  old_path: string | null;
}

/** Git 状态汇总 */
export interface GitStatus {
  branch: string;
  entries: GitStatusEntry[];
  ahead: number;
  behind: number;
}

/** 操作日志条目 */
export interface LogEntry {
  id: string;
  time: string;
  level: "info" | "warn" | "error" | "success";
  source: "agent" | "git" | "fs" | "system";
  message: string;
  details?: string;
}
