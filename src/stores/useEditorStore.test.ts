import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useEditorStore } from "./useEditorStore";

/** 内存版 Storage，够 persistEditorSession 用 */
function memoryStorage(): Storage {
  const data = new Map<string, string>();
  return {
    get length() {
      return data.size;
    },
    clear: () => data.clear(),
    getItem: (key: string) => data.get(key) ?? null,
    key: (index: number) => Array.from(data.keys())[index] ?? null,
    removeItem: (key: string) => void data.delete(key),
    setItem: (key: string, value: string) => void data.set(key, value),
  } as Storage;
}

beforeEach(() => {
  invokeMock.mockReset();
  const storage = memoryStorage();
  Object.assign(globalThis, {
    window: { __TAURI_INTERNALS__: {}, localStorage: storage },
    localStorage: storage,
  });
  useEditorStore.setState({
    openFiles: [],
    activeFile: null,
    fileContents: {},
    saveError: null,
  });
});

const tab = { path: "src/app.ts", name: "app.ts", isDirty: false, language: "typescript" };

describe("opening a file that cannot be read", () => {
  /**
   * 以前失败时把 `// Failed to load: <path>` 写进缓冲区。缓冲区是保存的事实来源，
   * 所以那行注释看起来像一个几乎空的真文件，接着按一次保存就覆盖了原文件。
   */
  it("does not put an error message into the buffer as if it were file content", async () => {
    invokeMock.mockRejectedValueOnce("EACCES: permission denied");

    await useEditorStore.getState().openFile(tab);

    const state = useEditorStore.getState();
    expect(state.fileContents[tab.path]).toBe("");
    expect(state.fileContents[tab.path]).not.toContain("Failed to load");
    expect(state.openFiles[0].loadError).toContain("permission denied");
  });

  it("refuses to save that tab, and says why", async () => {
    invokeMock.mockRejectedValueOnce("EACCES: permission denied");
    await useEditorStore.getState().openFile(tab);
    invokeMock.mockReset();

    await useEditorStore.getState().saveCurrentFile();

    // 关键断言：没有发出写请求
    expect(invokeMock).not.toHaveBeenCalled();
    const error = useEditorStore.getState().saveError ?? "";
    expect(error).toContain("app.ts");
    expect(error).toContain("permission denied");
  });

  it("clears the flag once the file reads successfully again", async () => {
    invokeMock.mockRejectedValueOnce("EACCES: permission denied");
    await useEditorStore.getState().openFile(tab);

    invokeMock.mockResolvedValueOnce("const value = 1;\n");
    await useEditorStore.getState().reloadFile(tab.path);

    expect(useEditorStore.getState().openFiles[0].loadError).toBeUndefined();
    expect(useEditorStore.getState().fileContents[tab.path]).toBe("const value = 1;\n");
  });
});

describe("saving", () => {
  it("reports a failed write instead of silently leaving the tab dirty", async () => {
    invokeMock.mockResolvedValueOnce("const value = 1;\n");
    await useEditorStore.getState().openFile(tab);
    useEditorStore.getState().updateFileContent(tab.path, "const value = 2;\n");
    expect(useEditorStore.getState().openFiles[0].isDirty).toBe(true);
    invokeMock.mockRejectedValueOnce("ENOSPC: no space left on device");

    await useEditorStore.getState().saveCurrentFile();

    expect(useEditorStore.getState().saveError).toContain("no space left");
    // 写失败了，dirty 标记必须留着 —— 清掉就等于告诉用户已经存好了
    expect(useEditorStore.getState().openFiles[0].isDirty).toBe(true);
  });

  it("clears a previous error after a successful save", async () => {
    invokeMock.mockResolvedValueOnce("const value = 1;\n");
    await useEditorStore.getState().openFile(tab);
    useEditorStore.setState({ saveError: "stale failure from an earlier attempt" });
    invokeMock.mockResolvedValueOnce(undefined);

    await useEditorStore.getState().saveCurrentFile();

    expect(useEditorStore.getState().saveError).toBeNull();
  });
});
