import { afterEach, describe, expect, it } from "vitest";
import { useLayoutStore } from "./useLayoutStore";

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
