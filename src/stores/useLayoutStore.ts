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

/**
 * 竖直方向上不属于任何面板的固定开销：TopBar 40 + 拖拽条 4 + StatusBar 24。
 * 底部面板之外只剩编辑器一列，所以编辑器列高 = 窗口高 - 这些 - bottomHeight。
 */
export const VERTICAL_CHROME = 68;
/**
 * 编辑器**这一列**至少要留下的高度 —— 不等于 Monaco 拿到的高度：`EditorTabs`
 * 约 29px，保存失败横幅出现时再吃掉约 28px，所以 200 落到代码区约 145px。
 * 窗口最小高度是 600（tauri.conf.json），底部面板上限 500 是个和窗口无关的字面量，
 * 两者一撞编辑器列只剩 32px，连标签栏都放不下。
 */
export const MIN_EDITOR_COLUMN_HEIGHT = 200;

/**
 * 当前窗口高度下，底部面板最多能有多高。纯函数、视口从参数进来：既能在 node
 * 环境的测试里直接算，也让"上限"只有这一处定义。
 */
export function maxBottomHeight(viewportHeight: number) {
  const room = viewportHeight - VERTICAL_CHROME - MIN_EDITOR_COLUMN_HEIGHT;
  // 下限 120 优先于上面那个承诺：窗口小到 388 以下时编辑器列会低于 200，
  // 但面板本身再压就没有内容区了。Tauri 的最小高度 600 保证不会走到这里，
  // 浏览器预览（npm run dev）和 devtools 停靠时可以。
  return Math.max(120, Math.min(500, Math.floor(room)));
}

/**
 * 尺寸的合法区间，读取存档和调整尺寸走同一套规则，避免两处漂移。
 *
 * `clampBottom` **刻意和窗口无关**：它是用户的意图，要原样存下来。窗口太小的时候
 * 由渲染侧取 `min(意图, maxBottomHeight(视口))`，这样缩小窗口不会把用户在大屏上
 * 拖出来的 500 永久改写成 372 —— 那是一次用户看不见、也无法撤销的偏好丢失。
 */
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

function snapshotOf(state: LayoutStore): PersistedLayout {
  return {
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
}

/** 已经写进 localStorage 的那份 JSON，用来跳过无变化的写入 */
let lastWritten: string | null = null;
let pendingWrite: ReturnType<typeof setTimeout> | null = null;

function writeNow() {
  if (pendingWrite !== null) {
    clearTimeout(pendingWrite);
    pendingWrite = null;
  }
  try {
    const serialized = JSON.stringify(snapshotOf(useLayoutStore.getState()));
    // 拖动到边界之后 clamp 会让后续每个事件都产生同一份快照
    if (serialized === lastWritten) return;
    localStorage.setItem(STORAGE_KEY, serialized);
    lastWritten = serialized;
  } catch {
    /* 存不进去（隐私模式、配额满）不该影响正常使用 */
  }
}

/**
 * 攒一下再写。
 *
 * 拖动面板时 `App.tsx` 每个 pointermove 都会调 setter，直接在订阅里写就是一次拖动
 * 几十到几百次同步 `localStorage.setItem` + `JSON.stringify`。用户只关心松手之后的
 * 结果，所以延后到最后一次变化之后再落盘。
 *
 * 代价是"最后一次改动可能没写完就退出"，所以页面隐藏和卸载时强制冲一次。
 */
const SAVE_DELAY_MS = 250;

function scheduleSave() {
  if (pendingWrite !== null) clearTimeout(pendingWrite);
  pendingWrite = setTimeout(writeNow, SAVE_DELAY_MS);
}


export const useLayoutStore = create<LayoutStore>((set) => ({
  ...loadLayout(),
  performanceOverlay: false,
  workspacePath: "",

  setLeftWidth: (w) => set({ leftWidth: clampLeft(w) }),
  setRightWidth: (w) => set({ rightWidth: clampRight(w) }),
  setBottomHeight: (h) => set({ bottomHeight: clampBottom(h) }),
  // 打开任何一个面板都必须退出 focus mode。
  //
  // 否则会留下"focusMode 为真、面板却开着"这种自相矛盾的组合，而它是持久化的：
  // 顶栏那个高亮的 Focus 按钮此时再点，走的是"退出"分支，于是**三个面板全部打开** ——
  // 用户按了一个写着"专注"的按钮，得到的是完整三栏布局。收起面板时不动 focusMode，
  // 因为那个方向不矛盾。
  toggleLeftPanel: () =>
    set((s) => ({ leftVisible: !s.leftVisible, focusMode: s.focusMode && s.leftVisible })),
  toggleRightPanel: () =>
    set((s) => ({ rightVisible: !s.rightVisible, focusMode: s.focusMode && s.rightVisible })),
  toggleBottomPanel: () =>
    set((s) => ({ bottomVisible: !s.bottomVisible, focusMode: s.focusMode && s.bottomVisible })),
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
useLayoutStore.subscribe(scheduleSave);

// 退出/切后台时把还欠着的那次写补上，否则松手就关窗会丢掉最后一次调整
if (typeof window !== "undefined") {
  window.addEventListener("pagehide", writeNow);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") writeNow();
  });
}

/** 立刻落盘，跳过防抖。测试用，也是上面两个事件处理器调用的同一条路径。 */
export function flushLayoutSave() {
  writeNow();
}
