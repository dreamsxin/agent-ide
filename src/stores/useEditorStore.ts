import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { FileTab, InlineSuggestion, DiffOverlay, IntentHint } from "../types/editor";
import type { FileMetadata, SearchResult } from "../types/project";
import { fileNameFromPath, normalizeFilePath, pathKey, pathsEqual } from "../utils/paths";
import { isTauriRuntime } from "../utils/tauri";

interface EditorStore {
  // 文件标签
  openFiles: FileTab[];
  activeFile: string | null;

  // 文件内容缓存 (path → content)
  fileContents: Record<string, string>;

  /**
   * 最近一次保存失败的原因，`null` 表示没有。
   *
   * 保存失败以前只走 `console.error`，界面上毫无反应 —— 用户以为存下去了。
   */
  saveError: string | null;
  clearSaveError: () => void;

  // Explorer 刷新触发器
  explorerKey: number;
  workspacePath: string;

  // AI 增强层
  inlineSuggestion: InlineSuggestion | null;
  diffOverlays: DiffOverlay[];
  intentHints: IntentHint[];

  // 选区
  selectedText: string | null;
  selectedRange: { startLine: number; endLine: number } | null;
  pendingRevealLocation: { file: string; line: number; column: number } | null;

  // ====== 文件打开/关闭 ======
  openFile: (tab: FileTab) => Promise<void>;
  closeFile: (path: string) => void;
  setActiveFile: (path: string) => void;
  markDirty: (path: string, dirty: boolean) => void;

  // ====== 文件内容操作 ======
  updateFileContent: (path: string, content: string) => void;
  saveCurrentFile: () => Promise<void>;
  reloadFile: (path: string) => Promise<void>;

  // Enhanced file tools
  copyPath: (src: string, dest: string) => Promise<void>;
  getFileMetadata: (path: string) => Promise<FileMetadata>;
  searchFiles: (root: string, pattern: string, maxDepth?: number) => Promise<SearchResult[]>;

  // ====== CRUD 操作 ======
  deletePath: (path: string) => Promise<void>;
  createFile: (path: string, content?: string) => Promise<void>;
  createDirectory: (path: string) => Promise<void>;
  renamePath: (oldPath: string, newPath: string) => Promise<void>;

  // ====== 文件监听 ======
  startWatching: () => Promise<void>;
  stopWatching: () => Promise<void>;

  // ====== 工作目录 ======
  setWorkspacePath: (path: string) => void;
  restoreEditorSession: (workspacePath: string) => Promise<void>;

  // ====== AI 增强层 ======
  setInlineSuggestion: (suggestion: InlineSuggestion | null) => void;
  addDiffOverlay: (overlay: DiffOverlay) => void;
  removeDiffOverlay: (file: string) => void;
  addIntentHint: (hint: IntentHint) => void;
  removeIntentHint: (line: number) => void;

  setSelectedText: (text: string | null) => void;
  setSelectedRange: (range: { startLine: number; endLine: number } | null) => void;
  revealLocation: (file: string, line: number, column: number) => void;
  clearPendingRevealLocation: () => void;
}

export const useEditorStore = create<EditorStore>((set, get) => ({
  openFiles: [],
  activeFile: null,
  fileContents: {},
  saveError: null,
  explorerKey: 0,
  workspacePath: "",
  inlineSuggestion: null,
  diffOverlays: [],
  intentHints: [],
  selectedText: null,
  selectedRange: null,
  pendingRevealLocation: null,

  openFile: async (tab) => {
    const normalizedTab = {
      ...tab,
      path: normalizeFilePath(tab.path),
      name: tab.name || fileNameFromPath(tab.path),
    };
    const s = get();
    const exists = s.openFiles.find((f) => pathsEqual(f.path, normalizedTab.path));
    if (exists) {
      set({ activeFile: exists.path });
      persistEditorSession();
      return;
    }

    let content = "";
    let loadError: string | undefined;
    try {
      content = isTauriRuntime()
        ? await invoke<string>("read_file_content", { path: normalizedTab.path })
        : `// File loading is available in the Tauri app runtime.\n// ${normalizedTab.path}`;
    } catch (err: unknown) {
      // 读取失败时缓冲区留空并记下原因，不再伪造一行注释当文件内容。
      // 缓冲区是保存的事实来源，伪造内容会在下一次保存时覆盖真文件。
      content = "";
      loadError = err instanceof Error ? err.message : String(err);
    }

    set((prev) => ({
      openFiles: [...prev.openFiles, { ...normalizedTab, loadError }],
      activeFile: normalizedTab.path,
      fileContents: { ...prev.fileContents, [normalizedTab.path]: content },
    }));
    persistEditorSession();
  },

  closeFile: (path) => {
    const key = pathKey(path);
    set((s) => {
      const closing = s.openFiles.find((f) => pathKey(f.path) === key);
      const remaining = s.openFiles.filter((f) => pathKey(f.path) !== key);
      const newActive =
        closing && s.activeFile && pathsEqual(s.activeFile, closing.path)
          ? remaining.length > 0
            ? remaining[remaining.length - 1].path
            : null
          : s.activeFile;
      const restContents = Object.fromEntries(
        Object.entries(s.fileContents).filter(([file]) => pathKey(file) !== key)
      );
      return {
        openFiles: remaining,
        activeFile: newActive,
        fileContents: restContents,
      };
    });
    persistEditorSession();
  },

  setActiveFile: (path) => {
    const existing = get().openFiles.find((file) => pathsEqual(file.path, path));
    set({ activeFile: existing?.path ?? normalizeFilePath(path) });
    persistEditorSession();
  },

  markDirty: (path, dirty) =>
    set((s) => ({
      openFiles: s.openFiles.map((f) =>
        f.path === path ? { ...f, isDirty: dirty } : f
      ),
    })),

  updateFileContent: (path, content) =>
    set((s) => {
      const existing = s.openFiles.find((file) => pathsEqual(file.path, path));
      const filePath = existing?.path ?? normalizeFilePath(path);
      const isDirty = content !== s.fileContents[filePath];
      return {
        fileContents: { ...s.fileContents, [filePath]: content },
        openFiles: s.openFiles.map((f) =>
          pathsEqual(f.path, filePath) ? { ...f, isDirty } : f
        ),
      };
    }),

  clearSaveError: () => set({ saveError: null }),

  saveCurrentFile: async () => {
    const { activeFile, fileContents, openFiles } = get();
    if (!activeFile) return;
    const content = fileContents[activeFile];
    if (content === undefined) return;
    if (!isTauriRuntime()) return;

    // 打开时读取失败过就不能保存：缓冲区里的内容不代表磁盘上的文件，
    // 写回去等于用一个空/残缺的缓冲区覆盖真文件。
    const tab = openFiles.find((file) => pathsEqual(file.path, activeFile));
    if (tab?.loadError) {
      set({
        saveError: `Not saved: ${tab.name} failed to load (${tab.loadError}), so the editor does not hold its real content. Close the tab and reopen it.`,
      });
      return;
    }

    try {
      await invoke("write_file_content", { path: activeFile, content });
      set((s) => ({
        saveError: null,
        openFiles: s.openFiles.map((f) =>
          f.path === activeFile ? { ...f, isDirty: false } : f
        ),
      }));
    } catch (e) {
      // 以前这里只有 console.error：保存失败在界面上毫无反应，
      // 而 dirty 标记也不会清除，看起来像"存了但还是脏的"。
      console.error("Failed to save file:", e);
      set({
        saveError: `Failed to save ${activeFile}: ${e instanceof Error ? e.message : String(e)}`,
      });
    }
  },

  reloadFile: async (path) => {
    if (!isTauriRuntime()) return;
    try {
      const content = await invoke<string>("read_file_content", { path });
      set((s) => ({
        fileContents: { ...s.fileContents, [path]: content },
        openFiles: s.openFiles.map((f) =>
          // 重新读成功就清掉 loadError，否则这个标签会永久停在"不可保存"
          f.path === path ? { ...f, isDirty: false, loadError: undefined } : f
        ),
      }));
    } catch (e) {
      console.error(`Failed to reload ${path}:`, e);
      set((s) => ({
        openFiles: s.openFiles.map((f) =>
          f.path === path
            ? { ...f, loadError: e instanceof Error ? e.message : String(e) }
            : f
        ),
      }));
    }
  },

  // ====== Enhanced file tools ======

  copyPath: async (src, dest) => {
    if (!isTauriRuntime()) return;
    await invoke("copy_path", { src, dest });
    set((prev) => ({ explorerKey: prev.explorerKey + 1 }));
  },

  getFileMetadata: async (path) => {
    if (!isTauriRuntime()) {
      throw new Error("File metadata is available in the Tauri app runtime.");
    }
    return await invoke<FileMetadata>("get_file_metadata", { path });
  },

  searchFiles: async (root, pattern, maxDepth) => {
    if (!isTauriRuntime()) return [];
    return await invoke<SearchResult[]>("search_files", { root, pattern, maxDepth: maxDepth ?? null });
  },

  // ====== CRUD ======

  deletePath: async (path) => {
    if (!isTauriRuntime()) return;
    await invoke("delete_path", { path });
    // 关闭该文件（如果是打开的文件）
    const s = get();
    if (s.openFiles.some((f) => f.path === path)) {
      s.closeFile(path);
    }
    // 刷新 Explorer
    set((prev) => ({ explorerKey: prev.explorerKey + 1 }));
  },

  createFile: async (path, content = "") => {
    if (!isTauriRuntime()) return;
    await invoke("create_file", { path, content });
    set((prev) => ({ explorerKey: prev.explorerKey + 1 }));
  },

  createDirectory: async (path) => {
    if (!isTauriRuntime()) return;
    await invoke("create_directory", { path });
    set((prev) => ({ explorerKey: prev.explorerKey + 1 }));
  },

  renamePath: async (oldPath, newPath) => {
    if (!isTauriRuntime()) return;
    await invoke("rename_path", { oldPath, newPath });
    // 更新已打开的文件引用
    set((s) => ({
      openFiles: s.openFiles.map((f) =>
        f.path === oldPath
          ? { ...f, path: newPath, name: newPath.split(/[/\\]/).pop() || newPath }
          : f
      ),
      activeFile: s.activeFile === oldPath ? newPath : s.activeFile,
      fileContents: (() => {
        const contents = { ...s.fileContents };
        if (contents[oldPath] !== undefined) {
          contents[newPath] = contents[oldPath];
          delete contents[oldPath];
        }
        return contents;
      })(),
      explorerKey: s.explorerKey + 1,
    }));
    persistEditorSession();
  },

  // ====== 文件监听 ======

  startWatching: async () => {
    if (!isTauriRuntime()) return;
    try {
      await invoke("watch_start");
    } catch (e) {
      console.warn("Failed to start file watching:", e);
    }
  },

  stopWatching: async () => {
    if (!isTauriRuntime()) return;
    try {
      await invoke("watch_stop");
    } catch (e) {
      console.warn("Failed to stop file watching:", e);
    }
  },

  // ====== 工作目录 ======

  setWorkspacePath: (path) => {
    if (typeof window !== "undefined") {
      localStorage.setItem("agent-ide-workspace-path", path);
    }
    set((prev) => ({
      workspacePath: path,
      explorerKey: prev.explorerKey + 1,  // 触发 Explorer 刷新
    }));
  },

  restoreEditorSession: async (workspacePath) => {
    const saved = loadEditorSession();
    if (!saved || saved.workspacePath !== workspacePath || saved.openFiles.length === 0) return;

    const restoredFiles: FileTab[] = [];
    const restoredContents: Record<string, string> = {};
    for (const tab of saved.openFiles.slice(0, 20)) {
      try {
        const content = isTauriRuntime()
          ? await invoke<string>("read_file_content", { path: tab.path })
          : `// File loading is available in the Tauri app runtime.\n// ${tab.path}`;
        // 这一分支意味着读取成功了，所以要清掉存档里可能带着的旧 loadError，
        // 否则一个曾经读失败的标签恢复之后会永久停在"不可保存"
        restoredFiles.push({ ...tab, isDirty: false, loadError: undefined });
        restoredContents[tab.path] = content;
      } catch {
        // Skip files that no longer exist or cannot be read.
      }
    }

    if (restoredFiles.length === 0) return;
    const activeFile =
      saved.activeFile && restoredFiles.some((file) => file.path === saved.activeFile)
        ? saved.activeFile
        : restoredFiles[0].path;

    set({
      openFiles: restoredFiles,
      activeFile,
      fileContents: restoredContents,
    });
    persistEditorSession();
  },

  // ====== AI 增强层 ======

  setInlineSuggestion: (inlineSuggestion) => set({ inlineSuggestion }),
  addDiffOverlay: (overlay) =>
    set((s) => ({
      diffOverlays: [
        ...s.diffOverlays.filter((d) => d.file !== overlay.file),
        overlay,
      ],
    })),
  removeDiffOverlay: (file) =>
    set((s) => ({
      diffOverlays: s.diffOverlays.filter((d) => d.file !== file),
    })),
  addIntentHint: (hint) =>
    set((s) => ({
      intentHints: [
        ...s.intentHints.filter((h) => h.line !== hint.line),
        hint,
      ],
    })),
  removeIntentHint: (line) =>
    set((s) => ({
      intentHints: s.intentHints.filter((h) => h.line !== line),
    })),

  setSelectedText: (selectedText) => set({ selectedText }),
  setSelectedRange: (selectedRange) => set({ selectedRange }),
  revealLocation: (file, line, column) =>
    set({ pendingRevealLocation: { file, line, column } }),
  clearPendingRevealLocation: () => set({ pendingRevealLocation: null }),
}));

interface PersistedEditorSession {
  workspacePath: string;
  openFiles: FileTab[];
  activeFile: string | null;
}

const EDITOR_SESSION_KEY = "agent-ide-editor-session";

function persistEditorSession() {
  if (typeof window === "undefined") return;
  const { workspacePath, openFiles, activeFile } = useEditorStore.getState();
  const session: PersistedEditorSession = {
    workspacePath,
    // loadError 是"这一次打开失败了"，不是文件的属性：下次启动会重新读，
    // 存进去只会让一个恢复成功的标签带着过期的不可保存标记
    openFiles: openFiles.map((file) => ({ ...file, isDirty: false, loadError: undefined })),
    activeFile,
  };
  localStorage.setItem(EDITOR_SESSION_KEY, JSON.stringify(session));
}

function loadEditorSession(): PersistedEditorSession | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = localStorage.getItem(EDITOR_SESSION_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as PersistedEditorSession;
    if (!parsed || !Array.isArray(parsed.openFiles)) return null;
    return parsed;
  } catch {
    return null;
  }
}
