// @vitest-environment jsdom
//
// 这个对话框在两个 cycle 里是完全惰性的：挂在 `App.tsx` 上，读着一个没有生产者的
// `pendingConfirm`，派发着没人监听的两个 window 事件。所以这些测试钉的不是渲染，
// 而是"点了之后后端真的收到决定"这条链路。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import ConfirmDialog from "./ConfirmDialog";
import { useAgentStore } from "../../stores/useAgentStore";
import type { DestructiveOpConfirm } from "../../types/agent";

afterEach(cleanup);

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(true);
  // store 里 `resolveConfirm` 先看运行时：没有这个标记它连 invoke 都不会发
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  useAgentStore.setState({ pendingConfirm: null, error: null });
});

function request(overrides: Partial<DestructiveOpConfirm> = {}): DestructiveOpConfirm {
  return {
    id: "req-1",
    opType: "browser_open",
    title: "Open a page in Chrome",
    description: "The agent wants to open http://127.0.0.1:1420/index.html",
    detail: "Origin http://127.0.0.1:1420 is allowed for this run.",
    ...overrides,
  };
}

describe("ConfirmDialog", () => {
  it("没有挂起的请求时不渲染任何东西", () => {
    const { container } = render(<ConfirmDialog />);

    expect(container.firstChild).toBeNull();
  });

  /**
   * 用户要为一件具体的事签字，所以 URL 必须在框里看得见 —— 只说"要打开一个页面"
   * 的批准框，等于请他为一个他看不见的目标授权。
   */
  it("把将要发生的事写在框里", () => {
    useAgentStore.setState({ pendingConfirm: request() });

    render(<ConfirmDialog />);

    expect(screen.getByText(/index\.html/)).toBeTruthy();
    expect(screen.getByText(/127\.0\.0\.1:1420 is allowed/)).toBeTruthy();
  });

  it("批准会把 approved: true 送回后端，并收掉对话框", async () => {
    useAgentStore.setState({ pendingConfirm: request() });

    render(<ConfirmDialog />);
    screen.getByText("Approve").click();

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("resolve_agent_approval", {
        requestId: "req-1",
        approved: true,
      })
    );
    expect(useAgentStore.getState().pendingConfirm).toBeNull();
  });

  /**
   * 拒绝也必须**送回去**，不能只关窗口：后端那次调用还挂在那儿等，只关窗口的话
   * 它要等到超时才被拒 —— 用户点了 Deny，动作却还在两分钟的窗口里。
   */
  it("拒绝会把 approved: false 送回后端", async () => {
    useAgentStore.setState({ pendingConfirm: request() });

    render(<ConfirmDialog />);
    screen.getByText("Deny").click();

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("resolve_agent_approval", {
        requestId: "req-1",
        approved: false,
      })
    );
    expect(useAgentStore.getState().pendingConfirm).toBeNull();
  });

  /**
   * 两次点击只该有一个决定：后端那条请求在第一次点击时就被取走了，第二次送过去
   * 只会落空 —— 而如果此时已经排上了下一条请求，第二次点击就会替一件用户还没读过
   * 的动作作答。
   */
  it("连点两次只送一个决定", async () => {
    useAgentStore.setState({ pendingConfirm: request() });

    render(<ConfirmDialog />);
    const approve = screen.getByText("Approve");
    approve.click();
    approve.click();

    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
  });

  /**
   * 键盘可达是安全要求：只能用鼠标回答的授权关卡对键盘用户等于不存在。Esc 必须走
   * 拒绝 —— "随手关掉"绝不能等于同意。
   */
  it("Esc 走拒绝，而不是关掉窗口了事", async () => {
    useAgentStore.setState({ pendingConfirm: request() });

    render(<ConfirmDialog />);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("resolve_agent_approval", {
        requestId: "req-1",
        approved: false,
      })
    );
  });

  /** 焦点默认落在 Deny：连按回车不该变成一次授权 */
  it("默认焦点在 Deny 上", () => {
    useAgentStore.setState({ pendingConfirm: request() });

    render(<ConfirmDialog />);

    expect(document.activeElement?.textContent).toBe("Deny");
  });
});

describe("closeConfirm", () => {
  /**
   * 后端超时之后会发 `agent-approval-closed`。只有 id 对得上才收 —— 否则一条刚
   * 排上来的新请求会被上一条的关闭通知顺手关掉。
   */
  it("只收掉 id 对得上的那一条", () => {
    useAgentStore.setState({ pendingConfirm: request({ id: "req-2" }) });

    useAgentStore.getState().closeConfirm("req-1");
    expect(useAgentStore.getState().pendingConfirm?.id).toBe("req-2");

    useAgentStore.getState().closeConfirm("req-2");
    expect(useAgentStore.getState().pendingConfirm).toBeNull();
  });
});

describe("resolveConfirm 的送达结果", () => {
  /**
   * 后端说"没人在等了"（超时或 Stop 已经拒过）时，这一次点击什么都没授权。
   * 只写 console 的话，"点了批准"和"批准生效"在界面上完全一样 —— 而这个对话框的
   * 全部意义就是让用户知道他授权了什么。
   */
  it("迟到的决定要在界面上说出来，不能只进 console", async () => {
    invokeMock.mockResolvedValue(false);
    useAgentStore.setState({ pendingConfirm: request(), error: null });

    const heard = await useAgentStore.getState().resolveConfirm(true);

    expect(heard).toBe(false);
    expect(useAgentStore.getState().error).toContain("too late");
  });

  it("送不出去也要说", async () => {
    invokeMock.mockRejectedValue(new Error("bridge is gone"));
    useAgentStore.setState({ pendingConfirm: request(), error: null });

    const heard = await useAgentStore.getState().resolveConfirm(false);

    expect(heard).toBe(false);
    expect(useAgentStore.getState().error).toContain("bridge is gone");
  });

  /**
   * 后端同时可以挂多条（registry 是 map），前端只有一个槽。被顶掉的那条在后端还挂着，
   * 用户却再也看不到它 —— 至少要留一句，否则"运行卡了两分钟"查不出原因。
   */
  it("顶掉一条没人回答的请求时要留话", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    useAgentStore.setState({ pendingConfirm: request({ id: "req-1" }) });

    useAgentStore.getState().requestConfirm(request({ id: "req-2" }));

    expect(warn).toHaveBeenCalled();
    expect(useAgentStore.getState().pendingConfirm?.id).toBe("req-2");
    warn.mockRestore();
  });
});
