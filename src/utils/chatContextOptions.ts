import type { IdeRuntimeContextOptions } from "./agentRuntimeContext";

/** Chat 里每一项上下文来源的开关 */
export type ChatContextOptions = {
  activeFile: boolean;
  selection: boolean;
  openFiles: boolean;
  problems: boolean;
  failedTask: boolean;
  terminalOutput: boolean;
  logs: boolean;
  gitDiff: boolean;
  projectTree: boolean;
  projectMemory: boolean;
};

export const DEFAULT_CONTEXT_OPTIONS: ChatContextOptions = {
  activeFile: true,
  selection: true,
  openFiles: true,
  problems: true,
  failedTask: true,
  terminalOutput: true,
  logs: true,
  gitDiff: true,
  projectTree: true,
  projectMemory: true,
};

const CONTEXT_OPTIONS_KEY = "agent-ide-chat-context-options";

/**
 * 这几样从 ChatView 里搬出来，是因为"让 Agent 修"那条路径也必须读同一份开关。
 *
 * 它以前用的是硬编码的默认值：用户在 Chat 里关掉了终端和日志，从问题面板点"Fix with
 * Agent"却照样把两者发出去，而那个界面上没有任何地方显示这件事。开关只有一份来源，
 * 才不会出现"界面说没发、实际发了"。
 */
export function loadContextOptions(): ChatContextOptions {
  if (typeof window === "undefined") return DEFAULT_CONTEXT_OPTIONS;
  try {
    const workspacePath = localStorage.getItem("agent-ide-workspace-path") ?? "";
    const raw = localStorage.getItem(CONTEXT_OPTIONS_KEY);
    if (!raw) return DEFAULT_CONTEXT_OPTIONS;
    const parsed = JSON.parse(raw) as {
      workspacePath?: string;
      options?: Partial<ChatContextOptions>;
    };
    // 换了工作区就回到默认：上一个项目关掉 git diff 的决定，对新项目没有意义
    if (parsed.workspacePath && workspacePath && parsed.workspacePath !== workspacePath) {
      return DEFAULT_CONTEXT_OPTIONS;
    }
    return { ...DEFAULT_CONTEXT_OPTIONS, ...(parsed.options ?? {}) };
  } catch {
    return DEFAULT_CONTEXT_OPTIONS;
  }
}

export function persistContextOptions(options: ChatContextOptions) {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem(
      CONTEXT_OPTIONS_KEY,
      JSON.stringify({
        workspacePath: localStorage.getItem("agent-ide-workspace-path") ?? "",
        options,
      })
    );
  } catch {
    // 存不下就算了：这只是个偏好，丢了下次回到默认
  }
}

/** 十个开关里，真正属于"IDE 运行状况"那一段的四个 */
export function ideRuntimeOptionsFor(options: ChatContextOptions): IdeRuntimeContextOptions {
  return {
    includeFailedTask: options.failedTask,
    includeProblems: options.problems,
    includeTerminalOutput: options.terminalOutput,
    includeLogs: options.logs,
  };
}
