import { useCallback, useRef, useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import TopBar from "./components/layout/TopBar";
import LeftPanel from "./components/layout/LeftPanel";
import EditorContainer from "./components/editor/EditorContainer";
import AgentPanel from "./components/layout/AgentPanel";
import BottomPanel from "./components/layout/BottomPanel";
import ResizeHandle from "./components/layout/ResizeHandle";
import ShortcutsHelp from "./components/shared/ShortcutsHelp";
import ErrorBoundary from "./components/shared/ErrorBoundary";
import { useLayoutStore } from "./stores/useLayoutStore";
import { useEditorStore } from "./stores/useEditorStore";
import { useLogStore } from "./stores/useLogStore";
import { useAgentBridge } from "./hooks/useAgentBridge";
import useShortcuts from "./hooks/useShortcuts";
import { isTauriRuntime } from "./utils/tauri";

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

  // 启动时恢复上次的工作目录
  useEffect(() => {
    if (!isTauriRuntime()) return;
    invoke<string | null>("get_workspace_path").then((saved) => {
      if (saved && typeof saved === "string" && saved.length > 0) {
        console.log("[App] Restoring workspace:", saved);
        useLayoutStore.getState().setWorkspacePath(saved);
        useEditorStore.getState().setWorkspacePath(saved);
        useLogStore.getState().restoreLogs(saved);
        void useEditorStore.getState().restoreEditorSession(saved);
      } else {
        console.log("[App] No saved workspace found, starting empty");
      }
    }).catch((err) => {
      console.warn("[App] Failed to load workspace:", err);
    });
  }, []);

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

  return (
    <div className="h-screen flex flex-col bg-surface-base text-surface-text overflow-hidden">
      <ShortcutsHelp
        shortcuts={allShortcuts}
        visible={helpVisible}
        onClose={() => setHelpVisible(false)}
      />

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
            <EditorContainer />
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
    </div>
  );
}
