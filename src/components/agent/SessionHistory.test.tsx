// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import SessionHistory, { formatRelativeTime } from "./SessionHistory";
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
});


describe("formatRelativeTime", () => {
  /** 0 是"后端没给时间戳"，不能显示成 1970 年。 */
  it("does not pretend an absent timestamp is the epoch", () => {
    expect(formatRelativeTime(0)).toBe("unknown time");
  });

  it("counts in the largest unit that still reads as a number", () => {
    const now = 10_000_000_000;
    expect(formatRelativeTime(now - 30_000, now)).toBe("just now");
    expect(formatRelativeTime(now - 5 * 60_000, now)).toBe("5m ago");
    expect(formatRelativeTime(now - 3 * 3_600_000, now)).toBe("3h ago");
    expect(formatRelativeTime(now - 2 * 86_400_000, now)).toBe("2d ago");
  });
});
