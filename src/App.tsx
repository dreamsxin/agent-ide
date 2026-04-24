import { useCallback } from "react";
import TopBar from "./components/layout/TopBar";
import LeftPanel from "./components/layout/LeftPanel";
import EditorContainer from "./components/editor/EditorContainer";
import AgentPanel from "./components/layout/AgentPanel";
import BottomPanel from "./components/layout/BottomPanel";
import ResizeHandle from "./components/layout/ResizeHandle";
import { useLayoutStore } from "./stores/useLayoutStore";

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
      {/* ======== 顶部控制栏 ======== */}
      <div className="h-12 flex-shrink-0">
        <TopBar />
      </div>

      {/* ======== 中间三栏区域 ======== */}
      <div className="flex-1 flex min-h-0">
        {/* 左侧面板 */}
        {leftVisible && (
          <>
            <div style={{ width: `${leftWidth}px` }} className="flex-shrink-0">
              <LeftPanel />
            </div>
            <ResizeHandle direction="horizontal" onResize={onLeftResize} />
          </>
        )}

        {/* 编辑器核心区 */}
        <div className="flex-1 min-w-0">
          <EditorContainer />
        </div>

        {/* 右侧 Agent 面板 */}
        {rightVisible && (
          <>
            <ResizeHandle direction="horizontal" onResize={onRightResize} />
            <div style={{ width: `${rightWidth}px` }} className="flex-shrink-0">
              <AgentPanel />
            </div>
          </>
        )}
      </div>

      {/* 垂直分隔 + 底部面板 */}
      {bottomVisible && (
        <>
          <ResizeHandle direction="vertical" onResize={onBottomResize} />
          <div style={{ height: `${bottomHeight}px` }} className="flex-shrink-0">
            <BottomPanel />
          </div>
        </>
      )}
    </div>
  );
}
