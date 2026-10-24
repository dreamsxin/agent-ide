// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

import StatusBar from "./StatusBar";
import { useAgentStore } from "../../stores/useAgentStore";
import { llmTargetFingerprint, UNVERIFIED_LLM_CONNECTION } from "../../stores/llmConnection";
import { useGitStore } from "../../stores/useGitStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useProblemStore } from "../../stores/useProblemStore";
import { translate, useLocaleStore } from "../../i18n";

afterEach(cleanup);

beforeEach(() => {
  // 显式定语言：默认值来自 `navigator.language`，断言里不该赌 jsdom 给什么
  useLocaleStore.getState().setLocale("zh");
  useProblemStore.setState({ problems: [] });
  useGitStore.setState({ status: null });
  useLayoutStore.setState({
    bottomVisible: true,
    bottomTab: "terminal",
    leftVisible: true,
    leftTab: "explorer",
  });
});

function problem(id: string, severity: "error" | "warning" | "info") {
  return {
    id,
    file: "src/app.ts",
    line: 1,
    column: 1,
    severity,
    source: "diagnostic" as const,
    message: "boom",
  };
}

describe("problems segment", () => {
  it("counts by severity", () => {
    useProblemStore.setState({
      problems: [problem("a", "error"), problem("b", "error"), problem("c", "warning")],
    });

    render(<StatusBar />);

    const label = screen.getByTestId("status-bar-problems").getAttribute("aria-label");
    // 断言的是"三个数进到了这句话里"，不是那句英文怎么写的。原来钉的是
    // "2 errors, 1 warning, 0 info" —— 那串是手写的单复数拼接，只有英文成立。
    expect(label).toContain(translate("zh", "status.problems", { errors: 2, warnings: 1, info: 0 }));
  });

  /**
   * 字母不是装饰。只靠颜色区分三个数字，色觉障碍的用户看到的是三个裸整数，
   * 分不清哪个是错误 —— 而 `aria-label` 对这类用户毫无帮助。
   */
  it("labels each count with a letter, not only a colour", () => {
    useProblemStore.setState({
      problems: [problem("a", "error"), problem("b", "error"), problem("c", "warning")],
    });

    render(<StatusBar />);

    expect(screen.getByTestId("status-bar-problems").textContent).toBe("E2W1I0");
  });

  it("reveals the Problems panel when the bottom panel is collapsed", () => {
    useLayoutStore.setState({ bottomVisible: false, bottomTab: "terminal" });

    render(<StatusBar />);
    screen.getByTestId("status-bar-problems").click();

    expect(useLayoutStore.getState().bottomTab).toBe("problems");
    expect(useLayoutStore.getState().bottomVisible).toBe(true);
  });

  /**
   * 用 `toggleBottomPanel()` 实现"打开面板"是很自然的写法，而它在面板已经打开时
   * 会把面板**关掉** —— 点一个"去看问题"的按钮结果把视图收起来，是最容易写出
   * 又最难自己发现的那类 bug。
   */
  it("does not collapse an already-open bottom panel", () => {
    render(<StatusBar />);
    screen.getByTestId("status-bar-problems").click();

    expect(useLayoutStore.getState().bottomTab).toBe("problems");
    expect(useLayoutStore.getState().bottomVisible).toBe(true);
  });
});

describe("branch segment", () => {
  function gitStatus(overrides: Record<string, unknown> = {}) {
    return {
      branch: "main",
      entries: [],
      ahead: 0,
      behind: 0,
      upstream: "origin/main",
      branches: [],
      conflicts: [],
      ...overrides,
    };
  }

  /**
   * 没有 Git 状态时整段不渲染，而不是显示一个空壳或者 "no branch"。
   * 打开的目录不是仓库是完全正常的情况，不该在状态栏里留一个疑问。
   */
  it("is absent when there is no git status", () => {
    render(<StatusBar />);

    expect(screen.queryByTestId("status-bar-branch")).toBeNull();
  });

  it("shows dirty and divergence markers only when they apply", () => {
    useGitStore.setState({
      status: gitStatus({ entries: [{ file: "a.ts", staged: false }], ahead: 2 }),
    });

    render(<StatusBar />);
    const text = screen.getByTestId("status-bar-branch").textContent ?? "";

    expect(text).toContain("main");
    expect(text).toContain("*");
    expect(text).toContain("\u21912");
    // behind 是 0，就不该出现向下箭头
    expect(text).not.toContain("\u2193");
  });

  it("opens Source Control without collapsing an already-open left panel", () => {
    useGitStore.setState({ status: gitStatus() });

    render(<StatusBar />);
    screen.getByTestId("status-bar-branch").click();

    expect(useLayoutStore.getState().leftTab).toBe("git");
    expect(useLayoutStore.getState().leftVisible).toBe(true);
  });

  it("reveals a collapsed left panel", () => {
    useGitStore.setState({ status: gitStatus() });
    useLayoutStore.setState({ leftVisible: false });

    render(<StatusBar />);
    screen.getByTestId("status-bar-branch").click();

    expect(useLayoutStore.getState().leftTab).toBe("git");
    expect(useLayoutStore.getState().leftVisible).toBe(true);
  });
});

/**
 * 这一段以前只有两档："存过 profile" 就是绿点加 "LLM ready"。端点填错、key 过期、
 * 模型名不存在，它一概照绿 —— 用户点了发送才知道不通。中间那档就是为此存在的。
 */
describe("LLM segment", () => {
  beforeEach(() => {
    useAgentStore.setState({
      llmConfigured: false,
      llmConnection: UNVERIFIED_LLM_CONNECTION,
      llmProfiles: [],
      activeProfileId: "",
      chatProfileId: null,
      llmEndpoint: "https://api.example.com/v1/chat/completions",
      llmModel: "gpt-4o",
      apiKeyMasked: "sk-1****cdef",
    });
  });

  /** 当前目标的指纹；测过的结果必须带着它才算数 */
  function currentTarget() {
    return llmTargetFingerprint(useAgentStore.getState());
  }

  it("没有凭据时说没配置", () => {
    render(<StatusBar />);

    expect(screen.getByTestId("status-bar-llm").textContent).toContain("not configured");
  });

  it("配置了但没测过，不说 connected", () => {
    useAgentStore.setState({ llmConfigured: true });

    render(<StatusBar />);

    const segment = screen.getByTestId("status-bar-llm");
    expect(segment.textContent).not.toContain("connected");
    expect(segment.getAttribute("title")).toMatch(/not been reached/);
  });

  it("测通了才说 connected", () => {
    useAgentStore.setState({ llmConfigured: true });
    useAgentStore.setState({
      llmConnection: { status: "ok", checkedAt: 1, detail: "pong", target: currentTarget() },
    });

    render(<StatusBar />);

    expect(screen.getByTestId("status-bar-llm").textContent).toContain("connected");
  });

  it("测失败时把失败原因带在 title 上，而不是只留一个红点", () => {
    useAgentStore.setState({ llmConfigured: true });
    useAgentStore.setState({
      llmConnection: {
        status: "failed",
        checkedAt: 1,
        detail: "401 Unauthorized",
        target: currentTarget(),
      },
    });

    render(<StatusBar />);

    const segment = screen.getByTestId("status-bar-llm");
    expect(segment.textContent).toContain("unreachable");
    expect(segment.getAttribute("title")).toContain("401 Unauthorized");
  });

  // 测完之后端点被改掉：那份 ok 说的是另一个地址，绿点不能继续替它作保
  it("目标变了之后，测通过的结果不再显示为 connected", () => {
    useAgentStore.setState({ llmConfigured: true });
    useAgentStore.setState({
      llmConnection: { status: "ok", checkedAt: 1, detail: "pong", target: currentTarget() },
    });
    useAgentStore.setState({ llmEndpoint: "http://localhost:11434/v1/chat/completions" });

    render(<StatusBar />);

    expect(screen.getByTestId("status-bar-llm").textContent).not.toContain("connected");
  });
});

