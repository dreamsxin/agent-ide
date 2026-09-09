// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

import StatusBar from "./StatusBar";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useProblemStore } from "../../stores/useProblemStore";

afterEach(cleanup);

beforeEach(() => {
  useProblemStore.setState({ problems: [] });
  useLayoutStore.setState({ bottomVisible: true, bottomTab: "terminal" });
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
    expect(label).toContain("2 errors, 1 warning, 0 info");
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
