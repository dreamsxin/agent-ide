// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, renderHook } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { usePaletteCommands } from "./CommandPalette";
import { useLayoutStore } from "../../stores/useLayoutStore";

afterEach(cleanup);

function commands() {
  return renderHook(() => usePaletteCommands(() => {}, [])).result.current;
}

describe("palette coverage of the Agent panel", () => {
  /**
   * Pipeline 和 Settings 在面板上只有两个 8px 宽的纯图标按钮，也没有快捷键。
   * 命令面板是它们唯一带文字的入口 —— 漏掉就等于 provider 配置、权限、花费上限
   * 和 MCP 全都只能靠碰对图标才能找到。
   */
  it("can reach every Agent view, not just the three primary tabs", () => {
    const titles = commands().map((command) => command.title);

    expect(titles).toContain("Open Agent Task");
    expect(titles).toContain("Open Agent Plan");
    expect(titles).toContain("Review Agent Changes");
    expect(titles).toContain("Configure Agent Pipeline");
    expect(titles).toContain("Open Agent Settings");
  });

  it("finds Settings by what lives inside it, since MCP has no entry of its own", () => {
    const settings = commands().find((command) => command.id === "panel.agent.settings");

    expect(settings?.keywords).toContain("mcp");
    expect(settings?.keywords).toContain("spend cap");
    expect(settings?.keywords).toContain("api key");
  });

  it("opens the Agent panel when a view command runs while it is hidden", () => {
    useLayoutStore.setState({ rightVisible: false, agentView: "task" });

    const settings = commands().find((command) => command.id === "panel.agent.settings");
    settings?.run();

    expect(useLayoutStore.getState().agentView).toBe("settings");
    // 只切视图不展开面板的话，命令看起来没有反应
    expect(useLayoutStore.getState().rightVisible).toBe(true);
  });
});
