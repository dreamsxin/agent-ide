import { afterEach, describe, expect, it } from "vitest";
import {
  MIN_EDITOR_COLUMN_HEIGHT,
  VERTICAL_CHROME,
  maxBottomHeight,
  useLayoutStore,
} from "./useLayoutStore";

describe("layout navigation state", () => {
  afterEach(() => {
    const state = useLayoutStore.getState();
    state.setAgentView("task");
    if (!state.rightVisible) state.toggleRightPanel();
  });

  it("keeps the selected Agent view while the panel is toggled", () => {
    const state = useLayoutStore.getState();

    state.setAgentView("changes");
    state.toggleRightPanel();
    state.toggleRightPanel();

    expect(useLayoutStore.getState().agentView).toBe("changes");
  });
});

describe("performance overlay", () => {
  it("stays off until something turns it on", () => {
    // 这个浮层驱动一个常驻 rAF 循环，默认开着等于常态白烧一帧。
    expect(useLayoutStore.getState().performanceOverlay).toBe(false);
  });

  it("toggles both directions so the close button can actually close it", () => {
    useLayoutStore.getState().togglePerformanceOverlay();
    expect(useLayoutStore.getState().performanceOverlay).toBe(true);

    useLayoutStore.getState().togglePerformanceOverlay();
    expect(useLayoutStore.getState().performanceOverlay).toBe(false);
  });
});

describe("maxBottomHeight", () => {
  /**
   * 断言落在真正重要的性质上 —— "编辑器列至少还剩这么高" —— 而不是某个具体像素值。
   * 写死 372 的话，以后调 MIN_EDITOR_COLUMN_HEIGHT 会得到一条指着纯算术恒等式的
   * 失败信息，看不出哪个不变量被破坏了。
   */
  it("always leaves the editor column its minimum at the smallest allowed window", () => {
    // 600 = tauri.conf.json 里的 minHeight
    const max = maxBottomHeight(600);
    expect(600 - VERTICAL_CHROME - max).toBeGreaterThanOrEqual(MIN_EDITOR_COLUMN_HEIGHT);
    // 而且确实比固定上限收紧了，否则这条断言可以恒真通过
    expect(max).toBeLessThan(500);
  });

  it("keeps the fixed 500 ceiling when the window has room to spare", () => {
    expect(maxBottomHeight(1200)).toBe(500);
    expect(maxBottomHeight(900)).toBe(500);
  });

  it("never goes below the panel's own minimum, and stays an integer", () => {
    // 分数 DPI 缩放下 innerHeight 可以是小数；高度会被写进存档，不能带小数
    expect(Number.isInteger(maxBottomHeight(720.5))).toBe(true);
    // 浏览器预览里窗口可以小到 Tauri 不允许的尺寸：这时面板下限赢，编辑器被压
    expect(maxBottomHeight(300)).toBe(120);
  });
});

