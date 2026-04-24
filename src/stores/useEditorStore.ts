import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { FileTab, InlineSuggestion, DiffOverlay, IntentHint } from "../types/editor";

interface EditorStore {
  // 文件标签
  openFiles: FileTab[];
  activeFile: string | null;

  // 文件内容缓存 (path → content)
  fileContents: Record<string, string>;

  // Explorer 刷新触发器
  explorerKey: number;

  // AI 增强层
  inlineSuggestion: InlineSuggestion | null;
  diffOverlays: DiffOverlay[];
  intentHints: IntentHint[];

  // 选区
  selectedText: string | null;
  selectedRange: { startLine: number; endLine: number } | null;

  // ====== 文件打开/关闭 ======
  openFile: (tab: FileTab) => Promise<void>;
  closeFile: (path: string) => void;
  setActiveFile: (path: string) => void;
  markDirty: (path: string, dirty: boolean) => void;

  // ====== 文件内容操作 ======
  updateFileContent: (path: string, content: string) => void;
  saveCurrentFile: () => Promise<void>;
  reloadFile: (path: string) => Promise<void>;

  // ====== CRUD 操作 ======
  deletePath: (path: string) => Promise<void>;
  createFile: (path: string, content?: string) => Promise<void>;
  createDirectory: (path: string) => Promise<void>;
  renamePath: (oldPath: string, newPath: string) => Promise<void>;

  // ====== 文件监听 ======
  startWatching: () => Promise<void>;
  stopWatching: () => Promise<void>;

  // ====== AI 增强层 ======
  setInlineSuggestion: (suggestion: InlineSuggestion | null) => void;
  addDiffOverlay: (overlay: DiffOverlay) => void;
  removeDiffOverlay: (file: string) => void;
  addIntentHint: (hint: IntentHint) => void;
  removeIntentHint: (line: number) => void;

  setSelectedText: (text: string | null) => void;
  setSelectedRange: (range: { startLine: number; endLine: number } | null) => void;
}

export const useEditorStore = create<EditorStore>((set, get) => ({
  openFiles: [],
  activeFile: null,
  fileContents: {},
  explorerKey: 0,
  inlineSuggestion: null,
  diffOverlays: [],
  intentHints: [],
  selectedText: null,
  selectedRange: null,

  openFile: async (tab) => {
    const s = get();
    const exists = s.openFiles.find((f) => f.path === tab.path);
    if (exists) {
      set({ activeFile: tab.path });
      return;
    }

    let content = "";
    try {
      content = await invoke<string>("read_file_content", { path: tab.path });
    } catch {
      content = `// Failed to load: ${tab.path}`;
    }

    set((prev) => ({
      openFiles: [...prev.openFiles, tab],
      activeFile: tab.path,
      fileContents: { ...prev.fileContents, [tab.path]: content },
    }));
  },

  closeFile: (path) =>
    set((s) => {
      const remaining = s.openFiles.filter((f) => f.path !== path);
      const newActive =
        s.activeFile === path
          ? remaining.length > 0
            ? remaining[remaining.length - 1].path
            : null
          : s.activeFile;
      const { [path]: _, ...restContents } = s.fileContents;
      return {
        openFiles: remaining,
        activeFile: newActive,
        fileContents: restContents,
      };
    }),

  setActiveFile: (path) => set({ activeFile: path }),

  markDirty: (path, dirty) =>
    set((s) => ({
      openFiles: s.openFiles.map((f) =>
        f.path === path ? { ...f, isDirty: dirty } : f
      ),
    })),

  updateFileContent: (path, content) =>
    set((s) => {
      const isDirty = content !== s.fileContents[path];
      return {
        fileContents: { ...s.fileContents, [path]: content },
        openFiles: s.openFiles.map((f) =>
          f.path === path ? { ...f, isDirty } : f
        ),
      };
    }),

  saveCurrentFile: async () => {
    const { activeFile, fileContents } = get();
    if (!activeFile) return;
    const content = fileContents[activeFile];
    if (content === undefined) return;

    try {
      await invoke("write_file_content", { path: activeFile, content });
      set((s) => ({
        openFiles: s.openFiles.map((f) =>
          f.path === activeFile ? { ...f, isDirty: false } : f
        ),
      }));
    } catch (e) {
      console.error("Failed to save file:", e);
    }
  },

  reloadFile: async (path) => {
    try {
      const content = await invoke<string>("read_file_content", { path });
      set((s) => ({
        fileContents: { ...s.fileContents, [path]: content },
        openFiles: s.openFiles.map((f) =>
          f.path === path ? { ...f, isDirty: false } : f
        ),
      }));
    } catch (e) {
      console.error(`Failed to reload ${path}:`, e);
    }
  },

  // ====== CRUD ======

  deletePath: async (path) => {
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
    await invoke("create_file", { path, content });
    set((prev) => ({ explorerKey: prev.explorerKey + 1 }));
  },

  createDirectory: async (path) => {
    await invoke("create_directory", { path });
    set((prev) => ({ explorerKey: prev.explorerKey + 1 }));
  },

  renamePath: async (oldPath, newPath) => {
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
  },

  // ====== 文件监听 ======

  startWatching: async () => {
    try {
      await invoke("watch_start");
    } catch (e) {
      console.warn("Failed to start file watching:", e);
    }
  },

  stopWatching: async () => {
    try {
      await invoke("watch_stop");
    } catch (e) {
      console.warn("Failed to stop file watching:", e);
    }
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
}));
