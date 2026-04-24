import { useAgentStore } from "../../stores/useAgentStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import StatusDot from "../shared/StatusDot";
import ModeSwitch from "../shared/ModeSwitch";

export default function TopBar() {
  const agentState = useAgentStore((s) => s.state);
  const agentMode = useAgentStore((s) => s.mode);
  const setAgentMode = useAgentStore((s) => s.setMode);
  const focusMode = useLayoutStore((s) => s.focusMode);
  const toggleFocusMode = useLayoutStore((s) => s.toggleFocusMode);
  const toggleLeftPanel = useLayoutStore((s) => s.toggleLeftPanel);
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel);
  const toggleBottomPanel = useLayoutStore((s) => s.toggleBottomPanel);

  return (
    <div className="flex items-center justify-between px-4 border-b border-surface-border bg-surface-panel h-full no-select">
      {/* 左侧：Logo + 项目名称 */}
      <div className="flex items-center gap-3">
        <div className="flex items-center gap-2">
          <span className="text-accent-purple font-bold text-sm tracking-wide">
            ⬨ Agent IDE
          </span>
        </div>
        <span className="text-xs text-surface-muted">my-project</span>
      </div>

      {/* 中间：Agent 模式切换 */}
      <div className="flex items-center gap-3">
        <ModeSwitch mode={agentMode} onChange={setAgentMode} />
      </div>

      {/* 右侧：状态 + 控制按钮 */}
      <div className="flex items-center gap-3">
        <StatusDot state={agentState} />

        <button
          className="px-2.5 py-1 text-xs bg-accent-blue hover:bg-blue-700 text-white rounded transition-colors"
          title="Run Task"
        >
          ▶ Run
        </button>

        <button
          className="px-2.5 py-1 text-xs bg-red-600/70 hover:bg-red-600 text-white rounded transition-colors"
          title="Stop Agent"
        >
          ■ Stop
        </button>

        <div className="w-px h-4 bg-surface-border" />

        {/* 面板切换按钮组 */}
        <button
          onClick={toggleLeftPanel}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors"
          title="Toggle Explorer"
        >
          📁
        </button>
        <button
          onClick={toggleRightPanel}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors"
          title="Toggle Agent Panel"
        >
          🤖
        </button>
        <button
          onClick={toggleBottomPanel}
          className="text-xs text-surface-muted hover:text-surface-text transition-colors"
          title="Toggle Terminal"
        >
          ⬜
        </button>
        <button
          onClick={toggleFocusMode}
          className={`text-xs transition-colors ${
            focusMode ? "text-accent-purple" : "text-surface-muted hover:text-surface-text"
          }`}
          title="Focus Mode"
        >
          ⊡
        </button>
      </div>
    </div>
  );
}
