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
  const module = await import("./useLayoutStore");
  return module.useLayoutStore;
}

describe("layout persistence", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("starts from defaults when nothing was saved", async () => {
    const store = await loadStore();

    expect(store.getState().leftWidth).toBe(240);
    expect(store.getState().rightVisible).toBe(true);
    expect(store.getState().bottomTab).toBe("terminal");
  });

  it("restores a saved layout instead of resetting on every launch", async () => {
    const store = await loadStore({
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

    const state = store.getState();
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
    const store = await loadStore();

    store.getState().setLeftWidth(300);
    store.getState().setBottomTab("logs");

    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
    expect(saved.leftWidth).toBe(300);
    expect(saved.bottomTab).toBe("logs");
  });

  /**
   * 存档可能来自上一个版本，也可能被手工改过。一个越界宽度或不存在的 tab 名会让
   * 面板变成不可见或不可达，而用户不知道为什么 —— 坏字段必须退回默认值。
   */
  it("clamps out-of-range sizes rather than adopting them", async () => {
    const store = await loadStore({ leftWidth: 9999, rightWidth: 1, bottomHeight: -50 });

    const state = store.getState();
    expect(state.leftWidth).toBe(500);
    expect(state.rightWidth).toBe(280);
    expect(state.bottomHeight).toBe(120);
  });

  it("falls back to defaults for unknown tab and view names", async () => {
    const store = await loadStore({
      leftTab: "explorer-v2",
      bottomTab: 42,
      agentView: "settings-old",
    });

    const state = store.getState();
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
    const store = await loadStore();

    store.getState().togglePerformanceOverlay();
    expect(store.getState().performanceOverlay).toBe(true);

    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
    expect(saved.performanceOverlay).toBeUndefined();
  });
});
