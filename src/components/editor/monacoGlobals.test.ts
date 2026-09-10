// @vitest-environment jsdom
//
// 逐文件开启 jsdom：`useLayoutStore` 在模块加载时会碰 localStorage。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

import { runAgentSelectionAction, setCurrentEditor } from "./monacoGlobals";
import { useAgentStore } from "../../stores/useAgentStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useLogStore } from "../../stores/useLogStore";

/** 只实现 `runAgentSelectionAction` 会碰到的那几个方法 */
function fakeEditor(selectedText: string | null) {
  return {
    getSelection: () =>
      selectedText === null
        ? null
        : { isEmpty: () => false, startLineNumber: 3, endLineNumber: 4 },
    getModel: () => ({ getValueInRange: () => selectedText ?? "" }),
  } as unknown as import("monaco-editor").editor.IStandaloneCodeEditor;
}

describe("runAgentSelectionAction", () => {
  let sendPrompt: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    sendPrompt = vi.fn(async () => undefined);
    useAgentStore.setState({ state: "idle", sendPrompt, addMessage: vi.fn() } as never);
    useEditorStore.setState({ activeFile: "a.ts", fileContents: { "a.ts": "A" } } as never);
    useLogStore.setState({ logs: [] } as never);
  });

  afterEach(() => {
    setCurrentEditor(null);
  });

  // 这是 25 里那个 bug 的回归测试：注册发生一次，调用发生在很久以后，所以
  // "哪个文件" 必须现取。挂载后又切了一次 tab，发出去的必须是新的那个。
  it("sends the file that is active now, not the one active when the editor mounted", async () => {
    setCurrentEditor(fakeEditor("const x = 1;"));
    useEditorStore.setState({
      activeFile: "b.ts",
      fileContents: { "a.ts": "A", "b.ts": "B" },
    } as never);

    await runAgentSelectionAction("explain");

    expect(sendPrompt).toHaveBeenCalledTimes(1);
    const request = sendPrompt.mock.calls[0][0] as {
      activeFile?: string;
      activeFileContent?: string;
      contextFiles: string[];
    };
    expect(request.activeFile).toBe("b.ts");
    expect(request.activeFileContent).toBe("B");
    expect(request.contextFiles).toEqual(["b.ts"]);
  });

  it("does not send a second prompt while a run is live, and says why", async () => {
    setCurrentEditor(fakeEditor("const x = 1;"));
    useAgentStore.setState({ state: "acting" } as never);

    await runAgentSelectionAction("explain");

    expect(sendPrompt).not.toHaveBeenCalled();
    expect(useLogStore.getState().logs.some((entry) => entry.level === "warn")).toBe(true);
  });

  it("does nothing when there is no editor or no selected text", async () => {
    setCurrentEditor(null);
    await runAgentSelectionAction("explain");

    setCurrentEditor(fakeEditor(null));
    await runAgentSelectionAction("explain");

    setCurrentEditor(fakeEditor(""));
    await runAgentSelectionAction("explain");

    expect(sendPrompt).not.toHaveBeenCalled();
  });
});
