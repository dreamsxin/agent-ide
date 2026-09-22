import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useAgentStore } from "./useAgentStore";
import {
  llmIndicator,
  llmTargetFingerprint,
  UNVERIFIED_LLM_CONNECTION,
} from "./llmConnection";
import { permissionsForPreset } from "../types/agent";
import type { LlmProfile } from "../types/agent";

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
    permissions: permissionsForPreset("read-only"),
    llmConnection: UNVERIFIED_LLM_CONNECTION,
    llmProfiles: [],
    activeProfileId: "",
    llmEndpoint: "",
    llmModel: "",
    apiKeyMasked: "",
  });
});

/** 一个够用的 profile；连通性只关心 id / endpoint / model */
function testProfile(overrides: Partial<LlmProfile> & { id: string }): LlmProfile {
  return {
    name: overrides.id,
    provider: "openai",
    endpoint: "https://api.example.com/v1/chat/completions",
    api_key_masked: "sk-1****cdef",
    model: "gpt-4o",
    ...overrides,
  };
}

describe("testLlmConnection", () => {
  beforeEach(() => {
    useAgentStore.setState({
      llmConfigured: true,
      llmProfiles: [testProfile({ id: "p1" }), testProfile({ id: "p2", model: "gpt-4o-mini" })],
      activeProfileId: "p1",
      chatProfileId: "p1",
    });
  });

  it("记下测通的结果，连同被测的那个目标", async () => {
    invokeMock.mockResolvedValueOnce("pong");

    await useAgentStore.getState().testLlmConnection();

    const connection = useAgentStore.getState().llmConnection;
    expect(connection.status).toBe("ok");
    expect(connection.detail).toBe("pong");
    expect(connection.target).toContain("gpt-4o");
  });

  // 失败原本只是抛出去，被调用方变成一句转瞬即逝的提示；状态栏那个点继续说"ready"。
  it("失败也留痕，而且照样往上抛", async () => {
    invokeMock.mockRejectedValueOnce("401 Unauthorized");

    await expect(useAgentStore.getState().testLlmConnection()).rejects.toBeTruthy();

    const connection = useAgentStore.getState().llmConnection;
    expect(connection.status).toBe("failed");
    expect(connection.detail).toContain("401 Unauthorized");
  });

  it("换 chat profile 之后，上一次的 ok 不再替新目标作保", async () => {
    invokeMock.mockResolvedValueOnce("pong");
    await useAgentStore.getState().testLlmConnection();

    useAgentStore.getState().setChatProfileId("p2");

    // 判断在渲染处：结果留在 store 里，但它带着的目标已经和当前目标不一致
    const state = useAgentStore.getState();
    expect(llmIndicator(true, state.llmConnection, llmTargetFingerprint(state)).tone).toBe("warn");
  });

  it("目标没变的一次配置刷新不该把验证过的结果抹掉", async () => {
    invokeMock.mockResolvedValueOnce("pong");
    await useAgentStore.getState().testLlmConnection();

    invokeMock.mockResolvedValueOnce({
      endpoint: "https://api.example.com/v1/chat/completions",
      model: "gpt-4o",
      api_key_masked: "sk-1****cdef",
      context_compression: "focused",
      profiles: [testProfile({ id: "p1" }), testProfile({ id: "p2", model: "gpt-4o-mini" })],
      active_profile_id: "p1",
    });
    await useAgentStore.getState().fetchLlmConfig();

    const state = useAgentStore.getState();
    expect(llmIndicator(true, state.llmConnection, llmTargetFingerprint(state)).tone).toBe("ok");
  });

  it("同一个 profile 改了端点，验证过的结果作废", async () => {
    invokeMock.mockResolvedValueOnce("pong");
    await useAgentStore.getState().testLlmConnection();

    invokeMock.mockResolvedValueOnce({
      endpoint: "http://localhost:11434/v1/chat/completions",
      model: "gpt-4o",
      api_key_masked: "sk-1****cdef",
      context_compression: "focused",
      profiles: [
        testProfile({ id: "p1", endpoint: "http://localhost:11434/v1/chat/completions" }),
      ],
      active_profile_id: "p1",
    });
    await useAgentStore.getState().fetchLlmConfig();

    const state = useAgentStore.getState();
    expect(llmIndicator(true, state.llmConnection, llmTargetFingerprint(state)).tone).toBe("warn");
  });

  /**
   * 后端拿到不认识的 profileId 会静默退回列表里的第一个。留着一个已删除的 id，
   * 前端说的目标和实际打出去的目标就不是同一个 —— 连通性和每一次 prompt 都算错账。
   */
  it("删掉正在用的 profile 后，chatProfileId 不会停在那个死 id 上", async () => {
    invokeMock.mockResolvedValueOnce({
      profiles: [testProfile({ id: "p2", model: "gpt-4o-mini" })],
      active_profile_id: "p2",
      context_compression: "focused",
    });

    await useAgentStore.getState().deleteLlmProfile("p1");

    expect(useAgentStore.getState().chatProfileId).toBe("p2");
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

    useAgentStore.setState({ permissions: permissionsForPreset("read-only") });
    await useAgentStore.getState().sendPrompt({ prompt: "read-only preset" });
    const askRequest = invokeMock.mock.calls[0][1] as { request: Record<string, unknown> };

    expect(askRequest.request.toolApproval).toBe("auto_approved_only");
    expect(askRequest.request.allowFileCreate).toBe(false);

    invokeMock.mockClear();
    useAgentStore.setState({ permissions: permissionsForPreset("run-commands") });
    await useAgentStore.getState().sendPrompt({ prompt: "run-commands preset" });
    const autoRequest = invokeMock.mock.calls[0][1] as { request: Record<string, unknown> };

    // 只有授予命令执行权限才放开全部 MCP 工具
    expect(autoRequest.request.toolApproval).toBe("allow_all");
    expect(autoRequest.request.allowFileCreate).toBe(true);
    // 命令执行权限决定后端是否把项目检查命令暴露成 Agent 工具。
    // ask 预设必须是 false：否则模型能自己跑命令，而用户从未同意过。
    expect(askRequest.request.allowCommandRun).toBe(false);
    expect(autoRequest.request.allowCommandRun).toBe(true);
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

  // Plan 标题和运行摘要两处都读 `currentTask.title`，而在这之前没有任何代码写过它，
  // 所以那两处永远显示硬编码的字面量。
  it("names the task from the prompt so the header stops showing a literal", async () => {
    invokeMock.mockResolvedValue("ok");

    await useAgentStore.getState().sendPrompt({
      prompt: "  Add pagination to the users list\n只改后端\n",
    });

    const task = useAgentStore.getState().currentTask;
    expect(task?.title).toBe("Add pagination to the users list");
    // id 用这一轮的 run id，方便和后端日志对上
    expect(task?.id).toBe(useAgentStore.getState().agentRunId);
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

describe("refreshPendingUndo", () => {
  /**
   * Undo 按钮的显示条件必须来自后端，不能从 diff 状态推断：回滚栈在
   * orchestrator 内存里，进程重启就没了，推断会显示一个点下去必然失败的按钮。
   */
  it("records the checkpoint the backend reports", async () => {
    invokeMock.mockResolvedValueOnce({ label: "Auto-apply", files: ["src/a.ts", "src/b.ts"] });

    await useAgentStore.getState().refreshPendingUndo();

    expect(invokeMock).toHaveBeenCalledWith("pending_undo");
    expect(useAgentStore.getState().pendingUndo).toEqual({
      label: "Auto-apply",
      files: ["src/a.ts", "src/b.ts"],
    });
  });

  it("treats no checkpoint as no way back rather than leaving a stale one", async () => {
    useAgentStore.setState({ pendingUndo: { label: "Auto-apply", files: ["src/a.ts"] } });
    invokeMock.mockResolvedValueOnce(null);

    await useAgentStore.getState().refreshPendingUndo();

    expect(useAgentStore.getState().pendingUndo).toBeNull();
  });

  /**
   * 挂载之后撤销可用性由 `agent-state-changed` 的 payload 推送，不再回头查询 ——
   * 查询要抢 orchestrator 锁，而运行期间那把锁被整条流水线占着。
   */
  it("is driven by the event payload after mount, without another query", async () => {
    invokeMock.mockClear();

    useAgentStore.getState().setPendingUndo({ label: "Tool write", files: ["src/c.ts"] });

    expect(useAgentStore.getState().pendingUndo).toEqual({
      label: "Tool write",
      files: ["src/c.ts"],
    });
    expect(invokeMock).not.toHaveBeenCalled();

    useAgentStore.getState().setPendingUndo(null);
    expect(useAgentStore.getState().pendingUndo).toBeNull();
  });



  // 这是背景刷新，不是用户发起的动作：失败不该抢占 error 横幅
  it("does not overwrite the error banner when the query fails", async () => {
    useAgentStore.setState({ error: "apply failed for src/a.ts" });
    invokeMock.mockRejectedValueOnce("no orchestrator");

    await useAgentStore.getState().refreshPendingUndo();

    expect(useAgentStore.getState().pendingUndo).toBeNull();
    expect(useAgentStore.getState().error).toBe("apply failed for src/a.ts");
  });
});

describe("applyAllDiffs", () => {
  const pendingDiff = {
    id: "diff-1",
    file: "smoke.txt",
    status: "pending" as const,
    hunks: [],
  };

  it("explains the empty result instead of looking like a dead button", async () => {
    // 后端的 diff 只在内存里，前端从 localStorage 恢复。重启后界面还显示
    // Apply All (N)，后端手上是空的，apply 返回 0/0 —— 以前这里什么都不做。
    useAgentStore.setState({ diffs: [pendingDiff], error: null });
    invokeMock.mockResolvedValueOnce({ applied: [], failed: [] });

    const applied = await useAgentStore.getState().applyAllDiffs();

    expect(applied).toEqual([]);
    const error = useAgentStore.getState().error ?? "";
    expect(error).toContain("Nothing was applied");
    expect(error).toContain("re-run");
    // 状态不能被谎报成 applied
    expect(useAgentStore.getState().diffs[0].status).toBe("pending");
  });

  it("stays quiet when there was nothing to apply in the first place", async () => {
    useAgentStore.setState({ diffs: [], error: null });
    invokeMock.mockResolvedValueOnce({ applied: [], failed: [] });

    await useAgentStore.getState().applyAllDiffs();

    expect(useAgentStore.getState().error).toBeNull();
  });

  it("surfaces a rejected invoke instead of swallowing it", async () => {
    useAgentStore.setState({ diffs: [pendingDiff], error: null });
    invokeMock.mockRejectedValueOnce("Diff is stale: smoke.txt changed on disk");

    await useAgentStore.getState().applyAllDiffs();

    expect(useAgentStore.getState().error).toContain("changed on disk");
  });
});

describe("stopAgent", () => {
  /**
   * Stop 结束的是运行，不是已经产出的东西。以前这里连 steps 和 diffs 一起清：Agent 改过的
   * 文件还在磁盘上，审查区却空了 —— 那正是这个产品存在要防的那件事，而且按一次 Stop 就能复现。
   */
  it("keeps the plan and the pending changes", async () => {
    useAgentStore.setState({
      state: "acting",
      steps: [{ id: "s1", title: "edit a file", status: "doing", logs: [] }] as never,
      diffs: [{ id: "d1", file: "src/a.ts", hunks: [], status: "pending" }] as never,
      agentRunId: "run-1",
    });
    invokeMock.mockResolvedValueOnce("Agent stopped");

    await useAgentStore.getState().stopAgent();

    expect(invokeMock).toHaveBeenCalledWith("stop_agent");
    expect(useAgentStore.getState().state).toBe("idle");
    expect(useAgentStore.getState().steps).toHaveLength(1);
    expect(useAgentStore.getState().diffs).toHaveLength(1);
    // run id 要清掉：那次运行确实结束了，后续的协调不该再认它
    expect(useAgentStore.getState().agentRunId).toBeNull();
  });
});

describe("session switching", () => {
  /**
   * "New session" 只清前端的话，下一条提问仍然带着上一个任务的对话摘要进模型上下文 ——
   * 界面看着是全新开始，模型还在接着上一件事聊，而用户看不出来。
   */
  it("also clears the conversation the backend would feed to the next prompt", async () => {
    useAgentStore.setState({
      conversationTurns: [{ id: "turn-1", prompt: "old task", outcome: "did things" }] as never,
      currentTask: { id: "t1", title: "old", description: "", status: "running" } as never,
    });
    invokeMock.mockResolvedValueOnce({
      activeId: "session-2",
      sessions: [],
      warning: null,
      sessionsAreSaved: true,
    });

    await useAgentStore.getState().startNewSession();

    expect(invokeMock).toHaveBeenCalledWith("start_new_agent_session");
    expect(useAgentStore.getState().conversationTurns).toEqual([]);
    expect(useAgentStore.getState().currentTask).toBeNull();
    // 后端回的新会话 id 必须落进 store：列表靠它高亮"current"
    expect(useAgentStore.getState().activeSessionId).toBe("session-2");
  });


  /**
   * 后端拒绝（运行还在跑）时界面**不能**先清空：清了的话用户看到一个空的、空闲的新会话，
   * 而那次运行还带着旧上下文在跑，连 Stop 按钮都跟着消失了。
   */
  it("keeps the view intact when the backend refuses to switch sessions", async () => {
    useAgentStore.setState({
      conversationTurns: [{ id: "turn-1", prompt: "old task", outcome: "did things" }] as never,
      steps: [{ id: "s1", title: "running step", status: "doing" }] as never,
    });
    invokeMock.mockRejectedValueOnce("A run is still in flight.");

    await expect(useAgentStore.getState().startNewSession()).rejects.toBeTruthy();

    expect(useAgentStore.getState().conversationTurns).toHaveLength(1);
    expect(useAgentStore.getState().steps).toHaveLength(1);
    expect(useAgentStore.getState().error).toContain("Could not start a new session");
  });


  /**
   * 恢复一个历史会话只换回上下文。计划和审查区必须清空 —— 留着上一个会话的步骤，
   * 用户会对着一排"点了会报错"的按钮，而那正是"界面显示的和后端实际的不一致"。
   */
  it("resuming loads the turns back and does not keep the previous plan", async () => {
    useAgentStore.setState({
      steps: [{ id: "s1", title: "old step", status: "done" }] as never,
      conversationTurns: [],
    });
    invokeMock.mockResolvedValueOnce({
      id: "session-old",
      title: "refactor the parser",
      turns: [
        { id: "turn-7", prompt: "refactor the parser", outcome: "2 file(s) applied" },
        { id: "turn-8", prompt: "Ran step: add tests", outcome: "no file changes", derived: true },
      ],
    });
    // resumeSession 结束时会再拉一次列表
    invokeMock.mockResolvedValueOnce({ activeId: "session-old", sessions: [], warning: null });

    await useAgentStore.getState().resumeSession("session-old");

    expect(invokeMock).toHaveBeenCalledWith("resume_agent_session", { sessionId: "session-old" });
    expect(useAgentStore.getState().conversationTurns).toHaveLength(2);
    expect(useAgentStore.getState().steps).toEqual([]);
    expect(useAgentStore.getState().currentTask?.title).toBe("refactor the parser");
    expect(useAgentStore.getState().activeSessionId).toBe("session-old");
    // 派生轮不能画成用户消息：那等于告诉用户 `Ran step: ...` 是他自己打的
    const messages = useAgentStore.getState().messages;
    expect(messages.find((m) => m.id === "turn-7-prompt")?.role).toBe("user");
    expect(messages.find((m) => m.id === "turn-8-prompt")?.role).toBe("agent");
  });


  /**
   * 删掉正在用的那个会话之后，界面上那几轮必须跟着消失：后端已经换了新会话，
   * 聊天区还列着一段模型此刻根本看不到的历史，比空着更误导。
   */
  it("deleting the active session drops the turns it was showing", async () => {
    useAgentStore.setState({
      activeSessionId: "session-active",
      conversationTurns: [{ id: "turn-1", prompt: "a", outcome: "b" }] as never,
    });
    invokeMock.mockResolvedValueOnce({ activeId: "session-new", sessions: [], warning: null });

    await useAgentStore.getState().deleteSession("session-active");

    expect(invokeMock).toHaveBeenCalledWith("delete_agent_session", {
      sessionId: "session-active",
    });
    expect(useAgentStore.getState().conversationTurns).toEqual([]);
    expect(useAgentStore.getState().activeSessionId).toBe("session-new");
  });

  /**
   * 一条没有 id 的会话点下去会发一个空 id 给后端，用户看到的是"点了就报错"的列表项。
   * 归一化必须把它扔掉，而不是靠 `as` 把类型检查关掉。
   */
  it("drops session rows that have no id", async () => {
    invokeMock.mockResolvedValueOnce({
      activeId: "session-1",
      sessions: [
        { id: "session-1", title: "ok", updatedAt: 10, turnCount: 1, lastOutcome: "" },
        { title: "no id at all", updatedAt: 20, turnCount: 3, lastOutcome: "" },
      ],
      warning: null,
    });

    await useAgentStore.getState().loadSessions();

    expect(useAgentStore.getState().sessions.map((s) => s.id)).toEqual(["session-1"]);
  });
});


describe("restoreDiffs", () => {
  it("prefers the backend list over the persisted one", async () => {
    const backendDiff = { id: "diff-backend", file: "a.ts", status: "pending", hunks: [] };


    invokeMock.mockResolvedValueOnce([backendDiff]);

    await useAgentStore.getState().restoreDiffs("/tmp/ws");

    expect(invokeMock).toHaveBeenCalledWith("get_agent_diffs");
    expect(useAgentStore.getState().diffs).toEqual([backendDiff]);
  });

  it("warns when restored changes are orphaned by an empty backend", async () => {
    localStorage.setItem(
      "agent-ide-agent-diffs",
      JSON.stringify({
        workspacePath: "/tmp/ws",
        diffs: [{ id: "diff-1", file: "smoke.txt", status: "pending", hunks: [] }],
      })
    );
    invokeMock.mockResolvedValueOnce([]);

    await useAgentStore.getState().restoreDiffs("/tmp/ws");

    expect(useAgentStore.getState().error).toContain("cannot be applied");
  });
});

describe("restoreAgentSession", () => {
  // "edit" 曾经是第三档模式，现在不是合法值了。`?? "suggest"` 只挡 undefined，
  // 挡不住一个真实但已废弃的值 —— 它会一路进到 store，让分段控件渲染出一个
  // 哪一段都没选中的状态，而用户完全不知道自己处在什么权限下。
  it("drops a mode that no longer exists instead of restoring it verbatim", () => {
    localStorage.setItem(
      "agent-ide-agent-session",
      JSON.stringify({
        workspacePath: "/tmp/ws",
        mode: "edit",
        currentTask: { id: "run-1", title: "跟进上一轮" },
        steps: [],
        pipeline: [],
      })
    );

    useAgentStore.getState().restoreAgentSession("/tmp/ws");

    expect(useAgentStore.getState().mode).toBe("suggest");
  });

  it("keeps auto, because that one is a real privilege level", () => {
    localStorage.setItem(
      "agent-ide-agent-session",
      JSON.stringify({
        workspacePath: "/tmp/ws",
        mode: "auto",
        currentTask: { id: "run-1", title: "跟进上一轮" },
        steps: [],
        pipeline: [],
      })
    );

    useAgentStore.getState().restoreAgentSession("/tmp/ws");

    expect(useAgentStore.getState().mode).toBe("auto");
  });
});

describe("forgetEarlierExternalActions", () => {
  /**
   * 界面上剩下什么由后端那份唯一的记录说了算，所以清空之后必须重新读一遍 ——
   * 本地先删一遍的版本会在后端拒绝时显示成"已经清掉了"。
   */
  it("re-reads the record instead of trimming the local copy", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "forget_earlier_external_actions") return Promise.resolve(2);
      if (command === "get_agent_external_actions") {
        return Promise.resolve([
          { id: "mine", kind: "browser_open", target: "https://a.example" },
        ]);
      }
      return Promise.resolve(null);
    });
    useAgentStore.setState({
      externalActions: [
        {
          id: "old",
          timestamp: "",
          kind: "browser_open",
          target: "https://old.example",
          detail: "",
          runId: null,
          restored: true,
        },
      ],
    });

    await useAgentStore.getState().forgetEarlierExternalActions();

    const commands = invokeMock.mock.calls.map((call) => call[0]);
    expect(commands).toContain("forget_earlier_external_actions");
    expect(commands).toContain("get_agent_external_actions");
    expect(useAgentStore.getState().externalActions.map((a) => a.id)).toEqual(["mine"]);
  });

  it("surfaces a failure instead of pretending the records are gone", async () => {
    invokeMock.mockImplementation((command: string) =>
      command === "forget_earlier_external_actions"
        ? Promise.reject(new Error("the log could not be read"))
        : Promise.resolve([])
    );

    await useAgentStore.getState().forgetEarlierExternalActions();

    expect(useAgentStore.getState().error).toContain("could not be read");
  });
});

