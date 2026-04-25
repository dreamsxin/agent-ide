import { useCallback, useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import TopBar from "./components/layout/TopBar";
import LeftPanel from "./components/layout/LeftPanel";
import EditorContainer from "./components/editor/EditorContainer";
import AgentPanel from "./components/layout/AgentPanel";
import BottomPanel from "./components/layout/BottomPanel";
import ResizeHandle from "./components/layout/ResizeHandle";
import ShortcutsHelp from "./components/shared/ShortcutsHelp";
import { useLayoutStore } from "./stores/useLayoutStore";
import { useEditorStore } from "./stores/useEditorStore";
import { useAgentBridge } from "./hooks/useAgentBridge";
import useShortcuts from "./hooks/useShortcuts";

function AnimatedPanel({ visible, className = "", children }: { visible: boolean; className?: string; children: React.ReactNode }) {
  const [shouldRender, setShouldRender] = useState(visible);
  const [animClass, setAnimClass] = useState("");

  useEffect(() => {
    if (visible) {
      setShouldRender(true);
      requestAnimationFrame(() => setAnimClass("panel-enter"));
    } else {
      setAnimClass("");
      const timer = setTimeout(() => setShouldRender(false), 200);
      return () => clearTimeout(timer);
    }
  }, [visible]);

  if (!shouldRender) return null;

  return <div className={`${animClass} ${className}`}>{children}</div>;
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
    invoke<string | null>("get_workspace_path").then((saved) => {
      if (saved && typeof saved === "string" && saved.length > 0) {
        useLayoutStore.getState().setWorkspacePath(saved);
        useEditorStore.getState().setWorkspacePath(saved);
      }
    }).catch(() => {
      // 无历史记录或读取失败，保持空状态
    });
  }, []);

  const allShortcuts = [
    ...shortcuts,
    { id: "help", keys: "F1", label: "Shortcuts Help",
      group: "General", scope: "global" as const,
      handler: () => setHelpVisible((v) => !v) },
  ];

  const onLeftResize = useCallback(
    (delta: number) => setLeftWidth(leftWidth + delta),
    [leftWidth, setLeftWidth]
  );
  const onRightResize = useCallback(
    (delta: number) => setRightWidth(rightWidth - delta),
    [rightWidth, setRightWidth]
  );
  const onBottomResize = useCallback(
    (delta: number) => setBottomHeight(bottomHeight - delta),
    [bottomHeight, setBottomHeight]
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
          <EditorContainer />
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

      <AnimatedPanel visible={bottomVisible} className="flex-shrink-0">
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
