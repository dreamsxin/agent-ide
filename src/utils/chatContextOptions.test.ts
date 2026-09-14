// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_CONTEXT_OPTIONS,
  ideRuntimeOptionsFor,
  loadContextOptions,
  persistContextOptions,
} from "./chatContextOptions";

describe("chatContextOptions", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("round-trips the toggles the user set", () => {
    persistContextOptions({ ...DEFAULT_CONTEXT_OPTIONS, logs: false, terminalOutput: false });

    const loaded = loadContextOptions();

    expect(loaded.logs).toBe(false);
    expect(loaded.terminalOutput).toBe(false);
    expect(loaded.problems).toBe(true);
  });

  /** 上一个项目关掉 git diff 的决定，对新项目没有意义 */
  it("falls back to defaults when the workspace changed", () => {
    localStorage.setItem("agent-ide-workspace-path", "/old");
    persistContextOptions({ ...DEFAULT_CONTEXT_OPTIONS, gitDiff: false });
    localStorage.setItem("agent-ide-workspace-path", "/new");

    expect(loadContextOptions().gitDiff).toBe(true);
  });

  it("survives a corrupt entry instead of throwing into the caller", () => {
    localStorage.setItem("agent-ide-chat-context-options", "{not json");

    expect(loadContextOptions()).toEqual(DEFAULT_CONTEXT_OPTIONS);
  });

  /**
   * "让 Agent 修"那条路径读的就是这份映射。它以前用硬编码的默认值，于是用户关掉的
   * 终端和日志照样被发出去，而界面上没有任何地方说这件事。
   */
  it("maps only the four runtime toggles", () => {
    const options = ideRuntimeOptionsFor({
      ...DEFAULT_CONTEXT_OPTIONS,
      logs: false,
      problems: false,
      gitDiff: false,
    });

    expect(options).toEqual({
      includeFailedTask: true,
      includeProblems: false,
      includeTerminalOutput: true,
      includeLogs: false,
    });
  });
});
