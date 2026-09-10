import { lazy, Suspense, useCallback, useRef, useState, useEffect } from "react";
import TopBar from "./components/layout/TopBar";
import LeftPanel from "./components/layout/LeftPanel";
import AgentPanel from "./components/layout/AgentPanel";
import BottomPanel from "./components/layout/BottomPanel";
import StatusBar from "./components/layout/StatusBar";
import ResizeHandle from "./components/layout/ResizeHandle";
import ShortcutsHelp from "./components/shared/ShortcutsHelp";
import CommandPalette, { usePaletteCommands } from "./components/shared/CommandPalette";
import ConfirmDialog from "./components/agent/ConfirmDialog";
import ErrorBoundary from "./components/shared/ErrorBoundary";
import PanelLoading from "./components/shared/PanelLoading";
import { useLayoutStore } from "./stores/useLayoutStore";
import { useAgentBridge } from "./hooks/useAgentBridge";
import { useAppBootstrap } from "./hooks/useAppBootstrap";
import useShortcuts from "./hooks/useShortcuts";
import { useProjectTasks } from "./hooks/useProjectTasks";
import { useRunProjectTask } from "./hooks/useRunProjectTask";

const EditorContainer = lazy(() => import("./components/editor/EditorContainer"));

function AnimatedPanel({
  visible,
  className = "",
  keepMounted = false,
  children,
}: {
  visible: boolean;
  className?: string;
  keepMounted?: boolean;
  children: React.ReactNode;
}) {
  const [shouldRender, setShouldRender] = useState(visible);
  const [animClass, setAnimClass] = useState("");

  useEffect(() => {
    if (visible) {
      setShouldRender(true);
      requestAnimationFrame(() => setAnimClass("panel-enter"));
    } else if (keepMounted) {
      setAnimClass("");
      setShouldRender(true);
    } else {
      setAnimClass("");
      const timer = setTimeout(() => setShouldRender(false), 200);
      return () => clearTimeout(timer);
    }
  }, [visible, keepMounted]);

  if (!shouldRender) return null;

  return (
    <div className={`${animClass} ${className} ${visible ? "" : "hidden"}`}>
      {children}
    </div>
  );
}

export default function App() {
  const leftWidth = useLayoutStore((s) => s.leftWidth);
  const rightWidth = useLayoutStore((s) => s.rightWidth);
  const bottomHeight = useLayoutStore((s) => s.bottomHeight);
  const leftVisible = useLayoutStore((s) => s.leftVisible);
  const rightVisible = useLayoutStore((s) => s.rightVisible);
  const bottomVisible = useLayoutStore((s) => s.bottomVisible);
  const setLeftWidth = useLayoutStore((s) => s.setLeftWidth);
  const setRightWidth = useLayoutStore((s) => s.setRightWidth);
  const setBottomHeight = useLayoutStore((s) => s.setBottomHeight);
  const resizeStartRef = useRef({ left: leftWidth, right: rightWidth, bottom: bottomHeight });

  useAgentBridge();

  const { shortcuts } = useShortcuts();
  const [helpVisible, setHelpVisible] = useState(false);
  const [commandPaletteVisible, setCommandPaletteVisible] = useState(false);
  const { tasks } = useProjectTasks();
  const runProjectTask = useRunProjectTask();
  const paletteCommands = usePaletteCommands(runProjectTask, tasks);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "F1") {
        e.preventDefault();
        setHelpVisible((v) => !v);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  useEffect(() => {
    const handler = () => setHelpVisible((v) => !v);
    window.addEventListener("toggle-shortcuts-help", handler);
    return () => window.removeEventListener("toggle-shortcuts-help", handler);
  }, []);

  useEffect(() => {
    const handler = () => setCommandPaletteVisible((v) => !v);
    window.addEventListener("toggle-command-palette", handler);
    return () => window.removeEventListener("toggle-command-palette", handler);
  }, []);

  useAppBootstrap();

  const allShortcuts = [
    ...shortcuts,
    { id: "help", keys: "F1", label: "Shortcuts Help",
      group: "General", scope: "global" as const,
      handler: () => setHelpVisible((v) => !v) },
  ];

  const onLeftResize = useCallback(
    (delta: number, phase?: "start" | "move" | "end") => {
      if (phase === "start") {
        resizeStartRef.current.left = useLayoutStore.getState().leftWidth;
        return;
      }
      if (phase === "move") setLeftWidth(resizeStartRef.current.left + delta);
    },
    [setLeftWidth]
  );
  const onRightResize = useCallback(
    (delta: number, phase?: "start" | "move" | "end") => {
      if (phase === "start") {
        resizeStartRef.current.right = useLayoutStore.getState().rightWidth;
        return;
      }
      if (phase === "move") setRightWidth(resizeStartRef.current.right - delta);
    },
    [setRightWidth]
  );
  const onBottomResize = useCallback(
    (delta: number, phase?: "start" | "move" | "end") => {
      if (phase === "start") {
        resizeStartRef.current.bottom = useLayoutStore.getState().bottomHeight;
        return;
      }
      if (phase === "move") setBottomHeight(resizeStartRef.current.bottom - delta);
    },
    [setBottomHeight]
  );

  // 底部面板的高度上限跟着窗口高度走，所以窗口一变就要重新走一遍 clamp。写回
  // 同一个 setter 而不是另开一条路径：上限的定义只能有一处，否则拖拽合法、
  // 缩窗口不合法这种不一致马上就会出现。挂载时也跑一次 —— 存档里的高度是在大屏
  // 上存下来的，换到小窗口时第一帧编辑器就已经被压没了。
  useEffect(() => {
    const reclamp = () => setBottomHeight(useLayoutStore.getState().bottomHeight);
    reclamp();
    window.addEventListener("resize", reclamp);
    return () => window.removeEventListener("resize", reclamp);
  }, [setBottomHeight]);


  return (
    <div data-testid="app-root" className="h-screen flex flex-col bg-surface-base text-surface-text overflow-hidden">
      <ShortcutsHelp
        shortcuts={allShortcuts}
        visible={helpVisible}
        onClose={() => setHelpVisible(false)}
      />
      <CommandPalette
        visible={commandPaletteVisible}
        commands={paletteCommands}
        onClose={() => setCommandPaletteVisible(false)}
      />
      <ConfirmDialog />

      {/* 自定义标题栏 */}
      <TopBar />

      <div className="flex-1 flex min-h-0">
        <AnimatedPanel visible={leftVisible} className="h-full">
          <div className="flex h-full gap-0">
            <div style={{ width: `${leftWidth}px` }} className="flex-shrink-0 h-full">
              <LeftPanel />
            </div>
            <ResizeHandle direction="horizontal" onResize={onLeftResize} />
          </div>
        </AnimatedPanel>

        <div className="flex-1 min-w-0">
          <ErrorBoundary fallbackTitle="Editor failed to render">
            <Suspense fallback={<PanelLoading label="Loading editor" />}>
              <EditorContainer />
            </Suspense>
          </ErrorBoundary>
        </div>

        <AnimatedPanel visible={rightVisible} className="h-full">
          <div className="flex h-full gap-0">
            <ResizeHandle direction="horizontal" onResize={onRightResize} />
            <div style={{ width: `${rightWidth}px` }} className="flex-shrink-0 h-full">
              <AgentPanel />
            </div>
          </div>
        </AnimatedPanel>
      </div>

      <AnimatedPanel visible={bottomVisible} keepMounted className="flex-shrink-0">
        <div>
          <ResizeHandle direction="vertical" onResize={onBottomResize} />
          <div style={{ height: `${bottomHeight}px` }} className="flex-shrink-0">
            <BottomPanel />
          </div>
        </div>
      </AnimatedPanel>

      {/* 状态栏在底部面板**外面**：它不是面板，focus mode 收起的是面板 */}
      <StatusBar />
    </div>
  );
}
