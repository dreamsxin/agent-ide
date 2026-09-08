// @vitest-environment jsdom
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
// 用 fireEvent 而不是 user-event：后者没装，为一个点击引入新依赖不值得
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";



const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("../../utils/tauri", () => ({ isTauriRuntime: () => true }));
vi.mock("../../hooks/useProjectTasks", () => ({
  useProjectTasks: () => ({
    tasks: [
      {
        id: "test",
        label: "test",
        command: "npm test",
        description: "run tests",
        source: "workspace",
      },
    ],
    usingFallback: false,
    loading: false,
    error: null,
  }),
}));
vi.mock("../../hooks/useRunProjectTask", () => ({ useRunProjectTask: () => vi.fn() }));
const sendFixPrompt = vi.fn();
vi.mock("../../hooks/useFixWithAgent", () => ({
  useFixWithAgent: () => ({
    fixTaskFailure: vi.fn(),
    sendFixPrompt,
    isAgentBusy: false,
  }),
}));
vi.mock("../../stores/useTaskStore", () => ({
  useTaskStore: (selector: (state: unknown) => unknown) =>
    selector({
      lastTask: null,
      taskRuns: {},
      taskRunHistory: [],
      clearTaskRunHistory: vi.fn(),
    }),
}));

import TasksPanel from "./TasksPanel";

describe("TasksPanel auto repair", () => {
  beforeEach(() => {
    invoke.mockReset();
    sendFixPrompt.mockReset();
  });

  // 这个仓库没有 vitest setup 文件，RTL 的自动清理不会注册，
  // 不手动清理的话第二个用例会在上一个的 DOM 上再渲染一份，getByTestId 报"多个匹配"
  afterEach(() => {
    cleanup();
  });


  /// 这个按钮是有界修复循环唯一的用户入口：后端早就实现了，但在此之前没有任何
  /// 前端代码调用 `repair_workspace`，也就是说人用不上。
  it("calls repair_workspace with a bounded iteration budget", async () => {
    invoke.mockResolvedValue({
      iterations: 1,
      stopReason: "checks passed",
      checksFailed: false,
      results: [{ command: "npm test", exitCode: 0 }],
    });

    render(<TasksPanel />);
    fireEvent.click(screen.getByTestId("repair-all"));

    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    const [command, payload] = invoke.mock.calls[0] as [string, { request: unknown }];
    expect(command).toBe("repair_workspace");
    // 预算必须传：不传的话后端按 1 轮处理，而这个按钮承诺的是"多试几轮"
    expect(payload.request).toMatchObject({
      commands: ["npm test"],
      maxIterations: 2,
    });
    // 修复循环自己落盘，不该再走一遍 Fix with Agent 的提示词路径
    expect(sendFixPrompt).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.getByText(/Checks pass after 1 round\(s\)/)).toBeTruthy()
    );
  });

  /// 放弃时要说清是第几轮停的、为什么停 —— 否则用户只看到工作区变了，
  /// 不知道 Agent 试了几次。
  it("reports the round count and the reason when it gives up", async () => {
    invoke.mockResolvedValue({
      iterations: 2,
      stopReason: "iteration budget exhausted",
      checksFailed: true,
      results: [{ command: "npm test", exitCode: 1 }],
    });

    render(<TasksPanel />);
    fireEvent.click(screen.getByTestId("repair-all"));

    await waitFor(() =>
      expect(
        screen.getByText(/Repair gave up after 2 round\(s\): iteration budget exhausted/)
      ).toBeTruthy()
    );
  });

  /// 非 Auto 模式下后端会拒绝。那条拒绝理由要原样显示，不能在前端悄悄兜住
  /// 变成"修复失败"——用户需要知道该去切模式。
  it("surfaces the backend refusal instead of swallowing it", async () => {
    invoke.mockRejectedValue(
      "Automatic repair applies its own fixes, so it requires Auto mode."
    );

    render(<TasksPanel />);
    fireEvent.click(screen.getByTestId("repair-all"));

    await waitFor(() => expect(screen.getByText(/requires Auto mode/)).toBeTruthy());
  });
});
