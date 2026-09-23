// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import SessionHistory, { emptyStateKey, relativeTime } from "./SessionHistory";
import { useAgentStore } from "../../stores/useAgentStore";

afterEach(cleanup);

function seed(overrides: Parameters<typeof useAgentStore.setState>[0]) {
  useAgentStore.setState({
    sessions: [],
    activeSessionId: "",
    sessionWarning: null,
    sessionsAreSaved: true,
    isStreaming: false,
    loadSessions: async () => undefined,
    ...(overrides as object),
  } as never);
}


describe("SessionHistory", () => {
  /**
   * 这个面板存在的理由就是"新建会话和历史会话找不到入口"。所以第一条断言的是：
   * 两件事都能从这里做到，而且历史里每一行都有能认出它的信息。
   */
  it("lists the saved sessions with something to recognise them by", () => {
    seed({
      sessions: [
        {
          id: "session-1",
          title: "refactor the parser",
          updatedAt: Date.now() - 5 * 60 * 1000,
          turnCount: 3,
          lastOutcome: "2 file(s) applied",
        },
      ],
      activeSessionId: "session-2",
    });

    render(<SessionHistory />);

    expect(screen.getByText("refactor the parser")).toBeDefined();
    expect(screen.getByText(/5m ago/)).toBeDefined();
    expect(screen.getByText("2 file(s) applied")).toBeDefined();
    expect(screen.getByTestId("session-new")).toBeDefined();
  });

  /**
   * 后端会拒绝"运行中换会话"（那一轮历史会记到别的会话里）。拒绝必须出现在屏幕上 ——
   * 吞掉的话按钮看起来就是点了没反应。
   */
  it("shows the backend's refusal instead of silently doing nothing", async () => {
    const resumeSession = vi.fn().mockRejectedValue("A run is still in flight.");
    seed({
      sessions: [
        {
          id: "session-1",
          title: "older ask",
          updatedAt: Date.now(),
          turnCount: 1,
          lastOutcome: "",
        },
      ],
      activeSessionId: "session-2",
      resumeSession,
    });

    render(<SessionHistory />);
    fireEvent.click(screen.getByText("older ask"));
    await vi.waitFor(() => {
      expect(screen.getByText(/still in flight/)).toBeDefined();
    });
    expect(resumeSession).toHaveBeenCalledWith("session-1");
  });

  /** 写不进磁盘时这里必须说出来：那件事在界面上没有任何其他症状。 */
  it("surfaces the warning when the history cannot be written", () => {
    seed({ sessionWarning: "write: permission denied" });

    render(<SessionHistory />);

    expect(screen.getByText(/permission denied/)).toBeDefined();
  });

  /**
   * "还没聊过"和"这个环境根本不保存"在屏幕上一样是一个空列表，而后者意味着用户刚才那一问
   * 不会被记住。空状态必须说清是哪一种。
   */
  it("says why the list is empty when nothing can be saved", () => {
    seed({ sessionsAreSaved: false });

    render(<SessionHistory />);

    expect(screen.getByText(/not available in the browser preview|Open a workspace folder/)).toBeDefined();
  });

  /** 新建会话被拒绝（运行还在跑）时，这个面板也要看得见那句话。 */
  it("shows the refusal when starting a new session is rejected", async () => {
    const startNewSession = vi.fn().mockRejectedValue("A run is still in flight.");
    seed({ startNewSession });

    render(<SessionHistory />);
    fireEvent.click(screen.getByTestId("session-new"));

    await vi.waitFor(() => {
      expect(screen.getByText(/still in flight/)).toBeDefined();
    });
  });

  /**
   * 改名是就地编辑：点铅笔 → 输入框带着现有名字出现 → 回车保存。
   *
   * 输入框预填现有名字而不是空的：多数改名是微调（加一个单词），从空白开始等于每次都要重打。
   */
  it("renames a task in place, starting from its current name", async () => {
    const renameSession = vi.fn().mockResolvedValue(undefined);
    seed({
      sessions: [
        {
          id: "session-1",
          title: "refactor the parser",
          updatedAt: Date.now(),
          turnCount: 2,
          lastOutcome: "",
        },
      ],
      activeSessionId: "session-1",
      renameSession,
    });

    render(<SessionHistory />);
    fireEvent.click(screen.getByLabelText("Rename task refactor the parser"));
    const input = screen.getByLabelText("Rename task refactor the parser") as HTMLInputElement;
    expect(input.value).toBe("refactor the parser");
    fireEvent.change(input, { target: { value: "Parser cleanup" } });
    fireEvent.click(screen.getByLabelText("Save the new name"));

    await vi.waitFor(() => {
      expect(renameSession).toHaveBeenCalledWith("session-1", "Parser cleanup");
    });
    // 保存之后回到普通行，否则那一行会一直停在编辑态
    await vi.waitFor(() => {
      expect(screen.getByText("refactor the parser")).toBeDefined();
    });
  });

  /** 空名字不送出去：后端会拒，而"提交了一个空名字"在界面上看起来像成功。 */
  it("does not send an empty name", () => {
    const renameSession = vi.fn();
    seed({
      sessions: [
        { id: "session-1", title: "older ask", updatedAt: Date.now(), turnCount: 1, lastOutcome: "" },
      ],
      renameSession,
    });

    render(<SessionHistory />);
    fireEvent.click(screen.getByLabelText("Rename task older ask"));
    fireEvent.change(screen.getByLabelText("Rename task older ask"), {
      target: { value: "   " },
    });

    expect((screen.getByLabelText("Save the new name") as HTMLButtonElement).disabled).toBe(true);
    expect(renameSession).not.toHaveBeenCalled();
  });

  /**
   * 分叉和恢复是两个不同的动作，所以是两个不同的控件：恢复会把后续每一轮写进那次记录，
   * 分叉留着它不动。被拒绝（运行中）时同样要说出来。
   */
  it("forks a task through its own control and reports a refusal", async () => {
    const forkSession = vi.fn().mockRejectedValue("A run is still in flight.");
    seed({
      sessions: [
        { id: "session-1", title: "older ask", updatedAt: Date.now(), turnCount: 1, lastOutcome: "" },
      ],
      activeSessionId: "session-2",
      forkSession,
    });

    render(<SessionHistory />);
    fireEvent.click(screen.getByLabelText("Fork task older ask"));

    await vi.waitFor(() => {
      expect(screen.getByText(/still in flight/)).toBeDefined();
    });
    expect(forkSession).toHaveBeenCalledWith("session-1");
  });
});


describe("relativeTime", () => {
  /** 0 是"后端没给时间戳"，不能显示成 1970 年。 */
  it("does not pretend an absent timestamp is the epoch", () => {
    expect(relativeTime(0).key).toBe("session.time.unknown");
  });

  it("counts in the largest unit that still reads as a number", () => {
    const now = 10_000_000_000;
    expect(relativeTime(now - 30_000, now)).toEqual({ key: "session.time.now", count: 0 });
    expect(relativeTime(now - 5 * 60_000, now)).toEqual({ key: "session.time.minutes", count: 5 });
    expect(relativeTime(now - 3 * 3_600_000, now)).toEqual({ key: "session.time.hours", count: 3 });
    expect(relativeTime(now - 2 * 86_400_000, now)).toEqual({ key: "session.time.days", count: 2 });
  });

  /**
   * 浏览器预览里根本存不下来，这一条要压过"还没聊过"：后者会让用户以为刚才那一问存好了。
   * 测试跑在 jsdom 里，没有 Tauri，所以这里能验到的就是这条优先级。
   */
  it("puts the missing backend ahead of every other empty state", () => {
    expect(emptyStateKey(true)).toBe("session.empty.noBackend");
  });
});
