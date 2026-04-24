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
