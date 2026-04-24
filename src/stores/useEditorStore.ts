import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { FileTab, InlineSuggestion, DiffOverlay, IntentHint } from "../types/editor";

interface EditorStore {
  // 文件标签
  openFiles: FileTab[];
  activeFile: string | null;

  // 文件内容缓存 (path → content)
  fileContents: Record<string, string>;

  // AI 增强层
  inlineSuggestion: InlineSuggestion | null;
  diffOverlays: DiffOverlay[];
  intentHints: IntentHint[];

  // 选区
  selectedText: string | null;
  selectedRange: { startLine: number; endLine: number } | null;

  // Actions
  openFile: (tab: FileTab) => Promise<void>;
  closeFile: (path: string) => void;
  setActiveFile: (path: string) => void;
  markDirty: (path: string, dirty: boolean) => void;

  // 文件内容操作
  updateFileContent: (path: string, content: string) => void;
  saveCurrentFile: () => Promise<void>;

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

    // 尝试从 Tauri FS 加载文件内容
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
      // 检测是否有实际变更
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
