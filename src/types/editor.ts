/** 编辑器标签页 */
export interface FileTab {
  path: string;
  name: string;
  isDirty: boolean;
  language: string;
  /**
   * 打开这个文件时的读取错误。
   *
   * 有值就意味着缓冲区里的内容不代表磁盘上的文件，因此**不能保存**。
   * 以前读取失败会把 `// Failed to load: <path>` 当成文件内容塞进缓冲区，
   * 看起来像一个几乎空的真文件；缓冲区又是保存的事实来源，于是接着按一次保存
   * 就会用这行注释覆盖原文件。
   */
  loadError?: string;
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
