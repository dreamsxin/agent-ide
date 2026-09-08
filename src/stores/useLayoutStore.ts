import { create } from "zustand";

export type AgentViewId = "task" | "plan" | "changes" | "pipeline" | "settings";

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
  bottomTab: "terminal" | "commands" | "problems" | "logs";

  // Agent 面板当前视图
  agentView: AgentViewId;

  // 编辑器性能浮层。默认关闭：它驱动一个常驻 requestAnimationFrame 循环，
  // 每帧都 setState，属于开发期诊断工具，不应该在正常使用时开着。
  performanceOverlay: boolean;


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
  setAgentView: (view: AgentViewId) => void;
  togglePerformanceOverlay: () => void;
  setWorkspacePath: (path: string) => void;
}

const STORAGE_KEY = "agent-ide-layout";

const LEFT_TABS: LayoutStore["leftTab"][] = ["explorer", "git"];
const BOTTOM_TABS: LayoutStore["bottomTab"][] = ["terminal", "commands", "problems", "logs"];
const AGENT_VIEWS: AgentViewId[] = ["task", "plan", "changes", "pipeline", "settings"];

/** 尺寸的合法区间，读取存档和调整尺寸走同一套规则，避免两处漂移 */
const clampLeft = (w: number) => Math.max(180, Math.min(500, w));
const clampRight = (w: number) => Math.max(280, Math.min(600, w));
const clampBottom = (h: number) => Math.max(120, Math.min(500, h));

/** 会被记住的那部分布局状态 */
type PersistedLayout = Pick<
  LayoutStore,
  | "leftWidth"
  | "rightWidth"
  | "bottomHeight"
  | "leftVisible"
  | "rightVisible"
  | "bottomVisible"
  | "focusMode"
  | "leftTab"
  | "bottomTab"
  | "agentView"
>;

const DEFAULT_LAYOUT: PersistedLayout = {
  leftWidth: 240,
  rightWidth: 360,
  bottomHeight: 240,
  leftVisible: true,
  rightVisible: true,
  bottomVisible: true,
  focusMode: false,
  leftTab: "explorer",
  bottomTab: "terminal",
  agentView: "task",
};

function pickNumber(value: unknown, clamp: (n: number) => number, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? clamp(value) : fallback;
}

function pickBoolean(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function pickFrom<T extends string>(value: unknown, allowed: T[], fallback: T): T {
  return allowed.includes(value as T) ? (value as T) : fallback;
}

/**
 * 从 localStorage 读回布局。
 *
 * 每个字段单独校验而不是整体信任：这份 JSON 可能来自上一个版本，也可能被手工改过。
 * 一个越界的宽度或一个不存在的 tab 名会让面板变成不可见或不可达，而用户完全不知道
 * 为什么 —— 所以坏字段一律退回默认值，而不是原样采用。
 *
 * 不记住的两项：`performanceOverlay` 是开发期诊断浮层（常驻 rAF 循环，跨会话保留
 * 是个坑），`workspacePath` 已经由 `useEditorStore` 单独持久化，再存一份就是两个
 * 事实来源。
 */
function loadLayout(): PersistedLayout {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_LAYOUT;
    const stored = JSON.parse(raw) as Record<string, unknown>;
    return {
      leftWidth: pickNumber(stored.leftWidth, clampLeft, DEFAULT_LAYOUT.leftWidth),
      rightWidth: pickNumber(stored.rightWidth, clampRight, DEFAULT_LAYOUT.rightWidth),
      bottomHeight: pickNumber(stored.bottomHeight, clampBottom, DEFAULT_LAYOUT.bottomHeight),
      leftVisible: pickBoolean(stored.leftVisible, DEFAULT_LAYOUT.leftVisible),
      rightVisible: pickBoolean(stored.rightVisible, DEFAULT_LAYOUT.rightVisible),
      bottomVisible: pickBoolean(stored.bottomVisible, DEFAULT_LAYOUT.bottomVisible),
      focusMode: pickBoolean(stored.focusMode, DEFAULT_LAYOUT.focusMode),
      leftTab: pickFrom(stored.leftTab, LEFT_TABS, DEFAULT_LAYOUT.leftTab),
      bottomTab: pickFrom(stored.bottomTab, BOTTOM_TABS, DEFAULT_LAYOUT.bottomTab),
      agentView: pickFrom(stored.agentView, AGENT_VIEWS, DEFAULT_LAYOUT.agentView),
    };
  } catch {
    return DEFAULT_LAYOUT;
  }
}

function saveLayout(state: LayoutStore) {
  try {
    const snapshot: PersistedLayout = {
      leftWidth: state.leftWidth,
      rightWidth: state.rightWidth,
      bottomHeight: state.bottomHeight,
      leftVisible: state.leftVisible,
      rightVisible: state.rightVisible,
      bottomVisible: state.bottomVisible,
      focusMode: state.focusMode,
      leftTab: state.leftTab,
      bottomTab: state.bottomTab,
      agentView: state.agentView,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(snapshot));
  } catch {
    /* 存不进去（隐私模式、配额满）不该影响正常使用 */
  }
}

export const useLayoutStore = create<LayoutStore>((set) => ({
  ...loadLayout(),
  performanceOverlay: false,
  workspacePath: "",

  setLeftWidth: (w) => set({ leftWidth: clampLeft(w) }),
  setRightWidth: (w) => set({ rightWidth: clampRight(w) }),
  setBottomHeight: (h) => set({ bottomHeight: clampBottom(h) }),
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
  setAgentView: (agentView) => set({ agentView }),
  togglePerformanceOverlay: () => set((s) => ({ performanceOverlay: !s.performanceOverlay })),
  setWorkspacePath: (workspacePath) => set({ workspacePath }),
}));

// 订阅一次而不是在 10 个 action 里各写一遍 save：拖动尺寸、切 tab、开关面板都要
// 记住，逐个 action 加保存迟早漏掉一个，而漏掉的那个"有时记得有时不记得"最难查。
useLayoutStore.subscribe(saveLayout);
