/** 编辑器标签页 */
export interface FileTab {
  path: string;
  name: string;
  isDirty: boolean;
  language: string;
}

/** 内联建议 */
export interface InlineSuggestion {
  line: number;
  column: number;
  text: string;
}

/** Diff 覆盖层 */
export interface DiffOverlay {
  file: string;
  oldText: string;
  newText: string;
  startLine: number;
}

/** AI 意图提示 */
export interface IntentHint {
  line: number;
  message: string;
  type: "optimize" | "warning" | "info" | "security";
}

/** 文件树节点 */
export interface FileNode {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  children?: FileNode[];
}
