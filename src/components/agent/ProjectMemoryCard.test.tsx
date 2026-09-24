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

import ProjectMemoryCard, { projectMemoryMessage } from "./ProjectMemoryCard";
import { useAgentStore } from "../../stores/useAgentStore";
import { normalizeProjectMemoryInfo } from "../../types/agent";
import { translate } from "../../i18n";

afterEach(cleanup);

beforeEach(() => {
  invokeMock.mockReset();
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  useAgentStore.setState({ isStreaming: false } as never);
});

function info(overrides: Record<string, unknown> = {}) {
  return {
    workspaceOpen: true,
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
  /**
   * 什么都没打开时，后端的 `workspace_root()` 会退回到进程的当前目录 —— 于是会读到**别的**
   * 项目的 AGENTS.md，而界面照着它说"这个工作区有一份"。这条测试钉住那句话不许出现。
   */
  it("says to open a workspace instead of describing someone else's file", async () => {
    invokeMock.mockResolvedValue(info({ workspaceOpen: false, bytes: 7000 }));

    render(<ProjectMemoryCard />);

    await waitFor(() => {
      expect(screen.getByText(translate("en", "memory.noWorkspace"))).toBeDefined();
    });
    expect(screen.queryByText(/7000/)).toBeNull();
    // 没有可做的动作：那个按钮会把提示词指向一个用户没打开的目录
    expect(screen.queryByText(translate("en", "memory.update"))).toBeNull();
  });

  it("says a project has no memory file, and where it would go", async () => {
    invokeMock.mockResolvedValue(info({ exists: false, bytes: 0 }));

    render(<ProjectMemoryCard />);

    await waitFor(() => {
      // 说清它该在哪：用户要知道该去哪个路径新建，而不是自己猜工作区根在哪
      expect(
        screen.getByText(
          translate("en", "memory.missing", { path: "C:\\work\\project\\AGENTS.md" })
        )
      ).toBeDefined();
    });
    expect(screen.getByText(translate("en", "memory.draft"))).toBeDefined();
  });

  /**
   * 截断是这块界面唯一真正新增的信息：以前只有模型知道尾部被丢了，用户看到的是"规则写了但
   * Agent 不照做"，而那两个数字是他唯一能据以行动的东西。
   */
  it("says how much of an oversized file is actually sent", async () => {
    invokeMock.mockResolvedValue(info({ bytes: 9200 }));

    render(<ProjectMemoryCard />);

    await waitFor(() => {
      expect(
        screen.getByText(translate("en", "memory.truncated", { bytes: 9200, limit: 8000 }))
      ).toBeDefined();
    });
    expect(screen.getByText(translate("en", "memory.update"))).toBeDefined();
  });

  it("says the file fits without claiming it always reaches the model whole", async () => {
    invokeMock.mockResolvedValue(info());

    render(<ProjectMemoryCard />);

    // "装得下"是事实；"每次都完整发出去"不是 —— 上下文预算和聊天里的开关都能再削它
    await waitFor(() => {
      expect(
        screen.getByText(translate("en", "memory.fits", { bytes: 1200, limit: 8000 }))
      ).toBeDefined();
    });
  });

  /** 按钮发的必须是后端给的那段提示词：前端自己编一段就会和后端的上限、文件名说不到一起。 */
  it("sends the backend's own prompt instead of inventing one", async () => {
    invokeMock.mockResolvedValue(info({ exists: false, bytes: 0 }));
    const sendPrompt = vi.fn().mockResolvedValue(undefined);
    useAgentStore.setState({ sendPrompt } as never);

    render(<ProjectMemoryCard />);
    const label = translate("en", "memory.draft");
    await waitFor(() => {
      expect(screen.getByText(label)).toBeDefined();
    });
    screen.getByText(label).click();

    await waitFor(() => {
      expect(sendPrompt).toHaveBeenCalledWith({
        prompt: "Write the file for the next Agent that works here.",
      });
    });
    // 落地方式也要说清：文件是当成一次可拒绝的改动进审查区的
    expect(screen.getByText(translate("en", "memory.sent"))).toBeDefined();
  });

  /** 读不懂的载荷不能渲染成"没有项目记忆"：那会让用户去新建一个已经存在的文件。 */
  it("says it could not read the status rather than guessing", async () => {
    invokeMock.mockResolvedValue({ nonsense: true });

    render(<ProjectMemoryCard />);

    await waitFor(() => {
      expect(screen.getByText(translate("en", "memory.unreadable"))).toBeDefined();
    });
    expect(screen.queryByText(translate("en", "memory.draft"))).toBeNull();
  });
});

/**
 * 五种状态各对一个 key：钉的是"状态 → 说哪一句"，不是那一句怎么写。
 * 中英两张表各自表述，测措辞就等于把翻译也钉死。
 */
describe("projectMemoryMessage", () => {
  const base = {
    workspaceOpen: true,
    exists: true,
    path: "C:\\p\\AGENTS.md",
    bytes: 1200,
    limit: 8000,
    truncated: false,
    draftPrompt: "x",
  };

  it("tells the five states apart", () => {
    expect(projectMemoryMessage(null, true).key).toBe("memory.unreadable");
    expect(projectMemoryMessage(null, false).key).toBe("memory.loading");
    expect(projectMemoryMessage({ ...base, workspaceOpen: false }, false).key).toBe(
      "memory.noWorkspace"
    );
    expect(projectMemoryMessage({ ...base, exists: false }, false).key).toBe("memory.missing");
    expect(projectMemoryMessage({ ...base, truncated: true }, false).key).toBe("memory.truncated");
    expect(projectMemoryMessage(base, false).key).toBe("memory.fits");
  });

  /** 读不懂优先于一切：有陈旧的 `info` 在手上也不能拿它当现状说。 */
  it("reports an unreadable payload even when an older reading is still held", () => {
    expect(projectMemoryMessage(base, true).key).toBe("memory.unreadable");
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
