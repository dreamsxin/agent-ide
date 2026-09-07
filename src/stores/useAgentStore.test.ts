import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useAgentStore } from "./useAgentStore";
import { permissionsForPreset } from "../types/agent";

/** 内存版 Storage，够 persistAgentSession / currentWorkspacePath 用 */
function memoryStorage(): Storage {
  const data = new Map<string, string>();
  return {
    get length() {
      return data.size;
    },
    clear: () => data.clear(),
    getItem: (key: string) => data.get(key) ?? null,
    key: (index: number) => Array.from(data.keys())[index] ?? null,
    removeItem: (key: string) => void data.delete(key),
    setItem: (key: string, value: string) => void data.set(key, value),
  } as Storage;
}

beforeEach(() => {
  invokeMock.mockReset();
  // isTauriRuntime() 检查 window.__TAURI_INTERNALS__，测试里必须为真，
  // 否则 sendPrompt 会走"仅 Tauri 可用"的分支而不会真正发请求
  const storage = memoryStorage();
  Object.assign(globalThis, {
    window: { __TAURI_INTERNALS__: {}, sessionStorage: storage, localStorage: storage },
    sessionStorage: storage,
    localStorage: storage,
  });
  useAgentStore.setState({
    error: null,
    state: "idle",
    isStreaming: false,
    chatProfileId: null,
    permissions: permissionsForPreset("ask"),
  });
});

describe("sendPrompt", () => {
  it("captures the backend error message and enters the error state", async () => {
    invokeMock.mockRejectedValueOnce(
      "Credential not found or inaccessible: No matching entry found in secure storage"
    );

    await useAgentStore.getState().sendPrompt({ prompt: "Update smoke.txt" });

    const state = useAgentStore.getState();
    // 这个字段一度被赋值却没有任何组件读取，失败只显示一个 Retry 按钮
    expect(state.error).toContain("No matching entry found in secure storage");
    expect(state.state).toBe("error");
    expect(state.isStreaming).toBe(false);
  });

  it("derives the MCP tool policy and file-create permission from the preset", async () => {
    invokeMock.mockResolvedValue("ok");

    useAgentStore.setState({ permissions: permissionsForPreset("ask") });
    await useAgentStore.getState().sendPrompt({ prompt: "ask preset" });
    const askRequest = invokeMock.mock.calls[0][1] as { request: Record<string, unknown> };

    expect(askRequest.request.toolApproval).toBe("auto_approved_only");
    expect(askRequest.request.allowFileCreate).toBe(false);

    invokeMock.mockClear();
    useAgentStore.setState({ permissions: permissionsForPreset("auto") });
    await useAgentStore.getState().sendPrompt({ prompt: "auto preset" });
    const autoRequest = invokeMock.mock.calls[0][1] as { request: Record<string, unknown> };

    // 只有授予命令执行权限才放开全部 MCP 工具
    expect(autoRequest.request.toolApproval).toBe("allow_all");
    expect(autoRequest.request.allowFileCreate).toBe(true);
  });

  it("sends the prompt through the send_agent_prompt command", async () => {
    invokeMock.mockResolvedValue("ok");

    await useAgentStore.getState().sendPrompt({ prompt: "hello" });

    expect(invokeMock).toHaveBeenCalledWith(
      "send_agent_prompt",
      expect.objectContaining({
        request: expect.objectContaining({ prompt: "hello" }),
      })
    );
  });
});

describe("setActiveLlmProfile", () => {
  it("maps the profile response onto the masked key shown in settings", async () => {
    invokeMock.mockResolvedValueOnce({
      profiles: [
        {
          id: "default",
          name: "default",
          provider: "custom",
          endpoint: "mock://smoke",
          api_key_masked: "****",
          model: "mock-model",
        },
      ],
      active_profile_id: "default",
      context_compression: "focused",
    });

    await useAgentStore.getState().setActiveLlmProfile("default");

    const state = useAgentStore.getState();
    expect(state.apiKeyMasked).toBe("****");
    expect(state.llmEndpoint).toBe("mock://smoke");
    expect(state.llmConfigured).toBe(true);
  });

  it("surfaces 'not configured' verbatim so the UI can tell it apart from a real key", async () => {
    invokeMock.mockResolvedValueOnce({
      profiles: [
        {
          id: "default",
          name: "default",
          provider: "custom",
          endpoint: "mock://smoke",
          api_key_masked: "not configured",
          model: "mock-model",
        },
      ],
      active_profile_id: "default",
      context_compression: "focused",
    });

    await useAgentStore.getState().setActiveLlmProfile("default");

    // 前端必须把这个字符串当成"未保存"，而不是当成真值显示 (saved)
    expect(useAgentStore.getState().apiKeyMasked).toBe("not configured");
  });
});

describe("undoLastApply", () => {
  it("clears the error banner when every file is restored", async () => {
    useAgentStore.setState({ error: "stale failure from an earlier run" });
    invokeMock.mockResolvedValueOnce({
      label: "Apply file src/app.ts",
      restored: ["src/app.ts"],
      failed: [],
    });

    const ok = await useAgentStore.getState().undoLastApply();

    expect(ok).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("undo_last_apply");
    // 撤销成功后还留着上一次的错误横幅会让人以为撤销也失败了
    expect(useAgentStore.getState().error).toBeNull();
  });

  it("reports a partial restore instead of claiming success", async () => {
    invokeMock.mockResolvedValueOnce({
      label: "Auto-apply",
      restored: ["src/a.ts"],
      failed: ["src/b.ts: permission denied"],
    });

    const ok = await useAgentStore.getState().undoLastApply();

    // 部分恢复不能算成功：磁盘此刻处于两次状态的中间
    expect(ok).toBe(false);
    const error = useAgentStore.getState().error ?? "";
    expect(error).toContain("1");
    expect(error.toLowerCase()).toContain("restored");
  });

  it("surfaces the backend refusal when there is nothing to undo", async () => {
    invokeMock.mockRejectedValueOnce(
      "Nothing to undo: no applied change is recorded"
    );

    const ok = await useAgentStore.getState().undoLastApply();

    expect(ok).toBe(false);
    // 静默失败会让按钮看起来是坏的，而不是"没有可撤销的东西"
    expect(useAgentStore.getState().error).toContain("Nothing to undo");
  });
});
