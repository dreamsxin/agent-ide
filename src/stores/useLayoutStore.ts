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

  // 当前底部面板标签
  bottomTab: "terminal" | "logs" | "tests" | "actions";

  // Actions
  setLeftWidth: (w: number) => void;
  setRightWidth: (w: number) => void;
  setBottomHeight: (h: number) => void;
  toggleLeftPanel: () => void;
  toggleRightPanel: () => void;
  toggleBottomPanel: () => void;
  toggleFocusMode: () => void;
  setBottomTab: (tab: LayoutStore["bottomTab"]) => void;
}

export const useLayoutStore = create<LayoutStore>((set) => ({
  leftWidth: 240,
  rightWidth: 360,
  bottomHeight: 240,
  leftVisible: true,
  rightVisible: true,
  bottomVisible: true,
  focusMode: false,
  bottomTab: "terminal",

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
  setBottomTab: (bottomTab) => set({ bottomTab }),
}));
