import { create } from "zustand";

interface LayoutStore {
  // 面板尺寸
  leftWidth: number;
  rightWidth: number;
  bottomHeight: number;

  // 显示状态
  leftVisible: boolean;
  rightVisible: boolean;
  bottomVisible: boolean;

  // 专注模式
  focusMode: boolean;

  // 左侧面板标签
  leftTab: "explorer" | "git";

  // 当前底部面板标签
  bottomTab: "terminal" | "logs" | "tests" | "actions";

  // workspacePath
  workspacePath: string;

  // Actions
  setLeftWidth: (w: number) => void;
  setRightWidth: (w: number) => void;
  setBottomHeight: (h: number) => void;
  toggleLeftPanel: () => void;
  toggleRightPanel: () => void;
  toggleBottomPanel: () => void;
  toggleFocusMode: () => void;
  setLeftTab: (tab: LayoutStore["leftTab"]) => void;
  setBottomTab: (tab: LayoutStore["bottomTab"]) => void;
  setWorkspacePath: (path: string) => void;
}

export const useLayoutStore = create<LayoutStore>((set) => ({
  leftWidth: 240,
  rightWidth: 360,
  bottomHeight: 240,
  leftVisible: true,
  rightVisible: true,
  bottomVisible: true,
  focusMode: false,
  leftTab: "explorer",
  bottomTab: "terminal",
  workspacePath: "",

  setLeftWidth: (w) => set({ leftWidth: Math.max(180, Math.min(500, w)) }),
  setRightWidth: (w) => set({ rightWidth: Math.max(280, Math.min(600, w)) }),
  setBottomHeight: (h) => set({ bottomHeight: Math.max(120, Math.min(500, h)) }),
  toggleLeftPanel: () => set((s) => ({ leftVisible: !s.leftVisible })),
  toggleRightPanel: () => set((s) => ({ rightVisible: !s.rightVisible })),
  toggleBottomPanel: () => set((s) => ({ bottomVisible: !s.bottomVisible })),
  toggleFocusMode: () =>
    set((s) => {
      if (s.focusMode) {
        return { focusMode: false, leftVisible: true, rightVisible: true, bottomVisible: true };
      }
      return { focusMode: true, leftVisible: false, rightVisible: false, bottomVisible: false };
    }),
  setLeftTab: (leftTab) => set({ leftTab }),
  setBottomTab: (bottomTab) => set({ bottomTab }),
  setWorkspacePath: (workspacePath) => set({ workspacePath }),
}));
