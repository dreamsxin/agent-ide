// @vitest-environment jsdom
//
// 这些测试钉的是"答案真的送回了后端"，以及"没答案绝不会变成一个答案"——后者是这道
// 对话框唯一不能出错的地方：编出来的答案会被模型当成用户的偏好带到后面每一步。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import QuestionDialog from "./QuestionDialog";
import { useAgentStore } from "../../stores/useAgentStore";
import { normalizeAgentQuestion, type AgentQuestion } from "../../types/agent";

afterEach(cleanup);

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(true);
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  useAgentStore.setState({ pendingQuestion: null, pendingConfirm: null, error: null });
});

function question(overrides: Partial<AgentQuestion> = {}): AgentQuestion {
  return {
    id: "q-1",
    question: "Which store should the cache use?",
    options: ["Redis", "In-memory"],
    ...overrides,
  };
}

describe("QuestionDialog", () => {
  it("没有挂起的提问时不渲染任何东西", () => {
    const { container } = render(<QuestionDialog />);

    expect(container.firstChild).toBeNull();
  });

  it("选一项就把那一项送回后端，并收掉提问框", async () => {
    useAgentStore.setState({ pendingQuestion: question() });

    render(<QuestionDialog />);
    screen.getByText("In-memory").click();

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("answer_agent_question", {
        requestId: "q-1",
        answer: "In-memory",
      })
    );
    expect(useAgentStore.getState().pendingQuestion).toBeNull();
  });

  /**
   * 自由输入不是补充功能：模型给的选项是它想到的几种，只能在其中选等于让它的想象力
   * 当成用户的全部选项。
   */
  it("自己写的答案照样送回去", async () => {
    useAgentStore.setState({ pendingQuestion: question() });

    render(<QuestionDialog />);
    const input = screen.getByLabelText("Your own answer");
    fireEvent.change(input, { target: { value: "Neither — keep it in the database" } });
    expect((screen.getByText("Send") as HTMLButtonElement).disabled).toBe(false);
    screen.getByText("Send").click();

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("answer_agent_question", {
        requestId: "q-1",
        answer: "Neither — keep it in the database",
      })
    );
  });

  /**
   * Esc 和"Don't answer"都必须走**没有答案**这条路：随手关掉绝不能被记成某个选项，
   * 而后端那次调用还挂着，只关窗口的话它要白等到两分钟超时。
   */
  it("不回答会告诉后端，而且不会变成某个选项", async () => {
    useAgentStore.setState({ pendingQuestion: question() });

    render(<QuestionDialog />);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("resolve_agent_approval", {
        requestId: "q-1",
        approved: false,
      })
    );
    expect(invokeMock).not.toHaveBeenCalledWith("answer_agent_question", expect.anything());
    expect(useAgentStore.getState().pendingQuestion).toBeNull();
  });

  it("空答案不发", async () => {
    useAgentStore.setState({ pendingQuestion: question() });

    render(<QuestionDialog />);
    expect((screen.getByText("Send") as HTMLButtonElement).disabled).toBe(true);
    const heard = await useAgentStore.getState().answerQuestion("   ");

    expect(heard).toBe(false);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  /**
   * 批准框优先。两个框都是 `fixed inset-0 z-50`、两个 Esc 监听器都挂在 window 上 ——
   * 同时出现的话，一次 Esc 会既"不回答这道题"又"拒绝那次授权"，也就是一次按键否掉了
   * 一件用户还没读过的动作。
   */
  it("有批准在等时不显示，也不抢 Esc", async () => {
    useAgentStore.setState({
      pendingQuestion: question(),
      pendingConfirm: {
        id: "req-9",
        opType: "browser_open",
        title: "Open a page",
        description: "The agent wants to open example.com",
        detail: "",
      },
    });

    const { container } = render(<QuestionDialog />);
    expect(container.firstChild).toBeNull();

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await Promise.resolve();
    expect(invokeMock).not.toHaveBeenCalled();
    // 那道题还挂着：批准回答完之后它会显示出来
    expect(useAgentStore.getState().pendingQuestion?.id).toBe("q-1");
    useAgentStore.setState({ pendingConfirm: null });
  });

  /** 顶掉一条没人回答的提问要进界面，不能只进 console */
  it("被顶掉的提问要在界面上留一句", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    useAgentStore.setState({ pendingQuestion: question({ id: "q-1" }), error: null });

    useAgentStore.getState().requestQuestion(question({ id: "q-2" }));

    expect(warn).toHaveBeenCalled();
    expect(useAgentStore.getState().pendingQuestion?.id).toBe("q-2");
    expect(useAgentStore.getState().error).toContain("never answered");
    warn.mockRestore();
  });
});

describe("answerQuestion 的送达结果", () => {
  /**
   * 后端说"没人在等了"时，这个答案没被任何人收到 —— 用户以为自己替 Agent 定了方向，
   * 而 Agent 正按自己的判断往下走。
   */
  it("迟到的答案要在界面上说出来", async () => {
    invokeMock.mockResolvedValue(false);
    useAgentStore.setState({ pendingQuestion: question(), error: null });

    const heard = await useAgentStore.getState().answerQuestion("Redis");

    expect(heard).toBe(false);
    expect(useAgentStore.getState().error).toContain("too late");
  });

  /** 后端不等这道题了（超时 / Stop），只收 id 对得上的那一条 */
  it("closeQuestion 只收掉 id 对得上的那一条", () => {
    useAgentStore.setState({ pendingQuestion: question({ id: "q-2" }) });

    useAgentStore.getState().closeQuestion("q-1");
    expect(useAgentStore.getState().pendingQuestion?.id).toBe("q-2");

    useAgentStore.getState().closeQuestion("q-2");
    expect(useAgentStore.getState().pendingQuestion).toBeNull();
  });
});

describe("normalizeAgentQuestion", () => {
  /**
   * 读不懂的载荷宁可不显示：一个只有一个按钮的"选择题"点不出任何信息，而显示它会让
   * 用户以为自己回答了什么。缺字段返回 null，由 bridge 回一个"没答案"。
   */
  it("缺字段或选项不足两个都不显示", () => {
    expect(normalizeAgentQuestion(null)).toBeNull();
    expect(normalizeAgentQuestion({ id: "q", question: "" , options: ["a", "b"] })).toBeNull();
    expect(normalizeAgentQuestion({ id: "", question: "Pick", options: ["a", "b"] })).toBeNull();
    expect(normalizeAgentQuestion({ id: "q", question: "Pick", options: ["only"] })).toBeNull();
    expect(
      normalizeAgentQuestion({ id: "q", question: "Pick", options: ["a", "", "  "] })
    ).toBeNull();
  });

  it("正常载荷收成一道题，选项去掉空白", () => {
    const parsed = normalizeAgentQuestion({
      id: "q-9",
      question: "  Which one?  ",
      options: ["  Redis ", "SQLite", 7],
    });

    expect(parsed).toEqual({ id: "q-9", question: "Which one?", options: ["Redis", "SQLite"] });
  });
});
