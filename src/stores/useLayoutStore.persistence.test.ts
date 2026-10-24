// @vitest-environment jsdom
// 单独一个文件：布局持久化必须在有 localStorage 的环境里测，而同目录的
// useLayoutStore.test.ts 跑在 node 环境下（那里 store 会静默退回默认值）。
import { beforeEach, describe, expect, it, vi } from "vitest";

const STORAGE_KEY = "agent-ide-layout";

/** store 在模块加载时读取存档，所以每个用例都要先写 localStorage 再重新 import */
async function loadStore(stored?: unknown) {
  localStorage.clear();
  if (stored !== undefined) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(stored));
  }
  vi.resetModules();
  return await import("./useLayoutStore");
}

function saved(): Record<string, unknown> {
  return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
}

describe("layout persistence", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("starts from defaults when nothing was saved", async () => {
    const { useLayoutStore } = await loadStore();

    expect(useLayoutStore.getState().leftWidth).toBe(240);
    expect(useLayoutStore.getState().rightVisible).toBe(true);
    expect(useLayoutStore.getState().bottomTab).toBe("terminal");
  });

  it("restores a saved layout instead of resetting on every launch", async () => {
    const { useLayoutStore } = await loadStore({
      leftWidth: 320,
      rightWidth: 420,
      bottomHeight: 180,
      leftVisible: false,
      rightVisible: true,
      bottomVisible: false,
      focusMode: false,
      leftTab: "git",
      bottomTab: "problems",
      agentView: "changes",
    });

    const state = useLayoutStore.getState();
    expect(state.leftWidth).toBe(320);
    expect(state.rightWidth).toBe(420);
    expect(state.bottomHeight).toBe(180);
    expect(state.leftVisible).toBe(false);
    expect(state.bottomVisible).toBe(false);
    expect(state.leftTab).toBe("git");
    expect(state.bottomTab).toBe("problems");
    expect(state.agentView).toBe("changes");
  });

  it("writes changes back so the next launch sees them", async () => {
    const { useLayoutStore, flushLayoutSave } = await loadStore();

    useLayoutStore.getState().setLeftWidth(300);
    useLayoutStore.getState().setBottomTab("logs");
    flushLayoutSave();

    expect(saved().leftWidth).toBe(300);
    expect(saved().bottomTab).toBe("logs");
  });

  /**
   * 拖动面板时 App.tsx 每个 pointermove 都会调 setter。直接在订阅里写，一次拖动
   * 就是几十到几百次同步 localStorage.setItem —— 用户只关心松手之后的结果。
   */
  it("does not write once per pointermove during a drag", async () => {
    const { useLayoutStore, flushLayoutSave } = await loadStore();
    const setItem = vi.spyOn(Storage.prototype, "setItem");

    for (let width = 200; width < 260; width += 1) {
      useLayoutStore.getState().setLeftWidth(width);
    }

    expect(setItem).not.toHaveBeenCalled();

    flushLayoutSave();
    expect(setItem).toHaveBeenCalledTimes(1);
    expect(saved().leftWidth).toBe(259);
    setItem.mockRestore();
  });

  /** clamp 之后越界拖动会反复产生同一份快照，那些写入没有意义 */
  it("skips a write when nothing actually changed", async () => {
    const { useLayoutStore, flushLayoutSave } = await loadStore();

    useLayoutStore.getState().setLeftWidth(9999);
    flushLayoutSave();
    const setItem = vi.spyOn(Storage.prototype, "setItem");

    useLayoutStore.getState().setLeftWidth(9999);
    flushLayoutSave();

    expect(setItem).not.toHaveBeenCalled();
    setItem.mockRestore();
  });

  /**
   * 存档可能来自上一个版本，也可能被手工改过。一个越界宽度或不存在的 tab 名会让
   * 面板变成不可见或不可达，而用户不知道为什么 —— 坏字段必须退回默认值。
   */
  it("clamps out-of-range sizes rather than adopting them", async () => {
    const { useLayoutStore } = await loadStore({
      leftWidth: 9999,
      rightWidth: 1,
      bottomHeight: -50,
    });

    const state = useLayoutStore.getState();
    expect(state.leftWidth).toBe(500);
    expect(state.rightWidth).toBe(280);
    expect(state.bottomHeight).toBe(120);
  });

  /**
   * 存档里存的是用户的意图，不是当前窗口下显示得出来的高度。窗口太小时的收窄由
   * 渲染侧负责（App.tsx 取 `min(意图, maxBottomHeight(视口))`）。如果在这里就把它
   * 改小，用户在大屏上拖出来的 500 会被永久改写，回到大屏也拿不回来 —— 一次用户
   * 看不见、也无法撤销的偏好丢失。
   */
  it("keeps a large saved height as the user's intent, whatever the window is", async () => {
    const original = window.innerHeight;
    Object.defineProperty(window, "innerHeight", { value: 600, configurable: true });
    try {
      const { useLayoutStore } = await loadStore({ bottomHeight: 500 });
      expect(useLayoutStore.getState().bottomHeight).toBe(500);
    } finally {
      Object.defineProperty(window, "innerHeight", { value: original, configurable: true });
    }
  });


  it("falls back to defaults for unknown tab and view names", async () => {
    const { useLayoutStore } = await loadStore({
      leftTab: "explorer-v2",
      bottomTab: 42,
      agentView: "settings-old",
    });

    const state = useLayoutStore.getState();
    expect(state.leftTab).toBe("explorer");
    expect(state.bottomTab).toBe("terminal");
    expect(state.agentView).toBe("task");
  });

  it("survives a corrupt entry without throwing", async () => {
    localStorage.clear();
    localStorage.setItem(STORAGE_KEY, "{not json");
    vi.resetModules();
    const { useLayoutStore } = await import("./useLayoutStore");

    expect(useLayoutStore.getState().leftWidth).toBe(240);
  });

  /**
   * 性能浮层驱动一个常驻 requestAnimationFrame 循环，每帧 setState。跨会话保留
   * 会让用户在完全不知情的情况下一直付这个开销。
   */
  it("does not remember the performance overlay", async () => {
    const { useLayoutStore, flushLayoutSave } = await loadStore();

    useLayoutStore.getState().togglePerformanceOverlay();
    expect(useLayoutStore.getState().performanceOverlay).toBe(true);
    flushLayoutSave();

    expect(saved().performanceOverlay).toBeUndefined();
  });

  /**
   * 打开面板必须退出 focus mode。
   *
   * 留下"focusMode 为真、面板却开着"的组合会让顶栏那个高亮的 Focus 按钮走
   * "退出"分支 —— 用户按一个写着专注的按钮，得到三栏全开。而这个矛盾状态
   * 是持久化的，重启还在。
   */
  it("leaves focus mode when a panel is opened", async () => {
    const { useLayoutStore } = await loadStore();

    useLayoutStore.getState().toggleFocusMode();
    expect(useLayoutStore.getState().focusMode).toBe(true);
    expect(useLayoutStore.getState().bottomVisible).toBe(false);

    useLayoutStore.getState().toggleBottomPanel();

    expect(useLayoutStore.getState().bottomVisible).toBe(true);
    expect(useLayoutStore.getState().focusMode).toBe(false);
  });

  it("stays in focus mode when a panel is closed", async () => {
    const { useLayoutStore } = await loadStore();

    useLayoutStore.setState({ focusMode: true, leftVisible: true });
    useLayoutStore.getState().toggleLeftPanel();

    // 收起面板和"专注"不矛盾，不该顺手把状态改掉
    expect(useLayoutStore.getState().leftVisible).toBe(false);
    expect(useLayoutStore.getState().focusMode).toBe(true);
  });
});
