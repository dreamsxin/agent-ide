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
