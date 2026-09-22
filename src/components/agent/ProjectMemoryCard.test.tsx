// @vitest-environment jsdom
//
// 这块卡片存在的理由是"项目记忆失效时屏幕上没有任何症状"，所以测试钉的正是那三种状态说的话
// 各不相同，以及那个按钮真的把后端给的提示词发出去了（而不是自己编一段）。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import ProjectMemoryCard from "./ProjectMemoryCard";
import { useAgentStore } from "../../stores/useAgentStore";
import { normalizeProjectMemoryInfo } from "../../types/agent";

afterEach(cleanup);

beforeEach(() => {
  invokeMock.mockReset();
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  useAgentStore.setState({ isStreaming: false } as never);
});

function info(overrides: Record<string, unknown> = {}) {
  return {
    exists: true,
    path: "C:\\work\\project\\AGENTS.md",
    bytes: 1200,
    limit: 8000,
    truncated: false,
    draftPrompt: "Write the file for the next Agent that works here.",
    ...overrides,
  };
}

describe("ProjectMemoryCard", () => {
  it("says a project has no memory file, and where it would go", async () => {
    invokeMock.mockResolvedValue(info({ exists: false, bytes: 0 }));

    render(<ProjectMemoryCard />);

    await waitFor(() => {
      expect(screen.getByText(/No AGENTS\.md in this workspace/)).toBeDefined();
    });
    // 说清它该在哪：用户要知道该去哪个路径新建，而不是自己猜工作区根在哪
    expect(screen.getByText(/C:\\work\\project\\AGENTS\.md/)).toBeDefined();
    expect(screen.getByText("Draft it with the Agent")).toBeDefined();
  });

  /**
   * 截断是这块界面唯一真正新增的信息：以前只有模型知道尾部被丢了，用户看到的是"规则写了但
   * Agent 不照做"，而那两个数字是他唯一能据以行动的东西。
   */
  it("says how much of an oversized file is actually sent", async () => {
    invokeMock.mockResolvedValue(info({ bytes: 9200 }));

    render(<ProjectMemoryCard />);

    await waitFor(() => {
      expect(screen.getByText(/9200 bytes/)).toBeDefined();
    });
    expect(screen.getByText(/only the first 8000/)).toBeDefined();
    expect(screen.getByText("Update it with the Agent")).toBeDefined();
  });

  it("says the whole file is sent when it fits", async () => {
    invokeMock.mockResolvedValue(info());

    render(<ProjectMemoryCard />);

    await waitFor(() => {
      expect(screen.getByText(/1200 of 8000 bytes/)).toBeDefined();
    });
    expect(screen.getByText(/all of it is sent/)).toBeDefined();
  });

  /** 按钮发的必须是后端给的那段提示词：前端自己编一段就会和后端的上限、文件名说不到一起。 */
  it("sends the backend's own prompt instead of inventing one", async () => {
    invokeMock.mockResolvedValue(info({ exists: false, bytes: 0 }));
    const sendPrompt = vi.fn().mockResolvedValue(undefined);
    useAgentStore.setState({ sendPrompt } as never);

    render(<ProjectMemoryCard />);
    await waitFor(() => {
      expect(screen.getByText("Draft it with the Agent")).toBeDefined();
    });
    screen.getByText("Draft it with the Agent").click();

    await waitFor(() => {
      expect(sendPrompt).toHaveBeenCalledWith({
        prompt: "Write the file for the next Agent that works here.",
      });
    });
    // 落地方式也要说清：文件是当成一次可拒绝的改动进审查区的
    expect(screen.getByText(/review area as a\s+normal change you can reject/)).toBeDefined();
  });

  /** 读不懂的载荷不能渲染成"没有项目记忆"：那会让用户去新建一个已经存在的文件。 */
  it("says it could not read the status rather than guessing", async () => {
    invokeMock.mockResolvedValue({ nonsense: true });

    render(<ProjectMemoryCard />);

    await waitFor(() => {
      expect(screen.getByText(/could not be read/)).toBeDefined();
    });
    expect(screen.queryByText("Draft it with the Agent")).toBeNull();
  });
});

describe("normalizeProjectMemoryInfo", () => {
  it("refuses a payload without the prompt or the path", () => {
    expect(normalizeProjectMemoryInfo(null)).toBeNull();
    expect(normalizeProjectMemoryInfo(info({ draftPrompt: "" }))).toBeNull();
    expect(normalizeProjectMemoryInfo(info({ path: "" }))).toBeNull();
    expect(normalizeProjectMemoryInfo(info({ limit: 0 }))).toBeNull();
  });

  /** 截断这件事自己算：和旁边显示的那两个数字算出同一个结论，才不会自相矛盾。 */
  it("derives truncation from the two numbers it shows", () => {
    const parsed = normalizeProjectMemoryInfo(info({ bytes: 9000, truncated: false }));

    expect(parsed?.truncated).toBe(true);
  });
});
