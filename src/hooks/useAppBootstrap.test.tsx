// @vitest-environment jsdom
//
// 逐文件开启 jsdom，而不是改全局 vitest 配置：其余 10 个测试文件都在 node
// 环境下跑得很好，没有理由为了这一个文件把它们的运行环境一起换掉。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useAppBootstrap } from "./useAppBootstrap";
import { useAgentStore } from "../stores/useAgentStore";

beforeEach(() => {
  invokeMock.mockReset();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("useAppBootstrap", () => {
  /// 这条正是之前漏掉的缺陷：`fetchLlmConfig` 一直是好的，坏的是启动时没人调它，
  /// 于是 TopBar 一直报 "LLM Not Configured" 直到用户打开一次 Agent 设置面板。
  /// store 层的测试看不到这种"挂载时机"问题。
  it("loads the LLM config on mount", async () => {
    const fetchLlmConfig = vi.fn().mockResolvedValue(undefined);
    useAgentStore.setState({ fetchLlmConfig });

    renderHook(() => useAppBootstrap());

    await waitFor(() => expect(fetchLlmConfig).toHaveBeenCalledTimes(1));
  });

  /// LLM 配置是全局的，和工作区无关：首次启动、从没保存过工作区时，指示灯
  /// 照样必须是准的。所以这两件事不能合进同一个 effect —— 工作区恢复那一支
  /// 在非 Tauri 运行时会直接 return。
  it("loads the LLM config even when there is no workspace to restore", async () => {
    const fetchLlmConfig = vi.fn().mockResolvedValue(undefined);
    useAgentStore.setState({ fetchLlmConfig });

    renderHook(() => useAppBootstrap());

    await waitFor(() => expect(fetchLlmConfig).toHaveBeenCalledTimes(1));
    // 浏览器预览模式下不该去问后端要工作区
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("restores the saved workspace in the Tauri runtime", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const fetchLlmConfig = vi.fn().mockResolvedValue(undefined);
    const restoreAgentSession = vi.fn();
    const restoreDiffs = vi.fn().mockResolvedValue(undefined);
    const reconcileBackendRun = vi.fn().mockResolvedValue(undefined);
    useAgentStore.setState({
      fetchLlmConfig,
      restoreAgentSession,
      restoreDiffs,
      reconcileBackendRun,
    });
    invokeMock.mockResolvedValue("D:/work/demo");

    renderHook(() => useAppBootstrap());

    await waitFor(() => expect(restoreAgentSession).toHaveBeenCalledWith("D:/work/demo"));
    expect(invokeMock).toHaveBeenCalledWith("get_workspace_path");
    expect(fetchLlmConfig).toHaveBeenCalledTimes(1);
  });

  /// 后端拿不到工作区时不能把启动流程带崩：这条路径以前只有一个 `.catch` 里的
  /// console.warn，没有任何测试碰过。
  it("survives a failing workspace lookup", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const fetchLlmConfig = vi.fn().mockResolvedValue(undefined);
    useAgentStore.setState({ fetchLlmConfig });
    invokeMock.mockRejectedValue(new Error("workspace unavailable"));

    expect(() => renderHook(() => useAppBootstrap())).not.toThrow();

    await waitFor(() => expect(fetchLlmConfig).toHaveBeenCalledTimes(1));
  });
});
