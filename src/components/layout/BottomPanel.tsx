import { useLayoutStore } from "../../stores/useLayoutStore";
import Terminal from "../panels/Terminal";
import LogView from "../panels/LogView";

type BottomTab = "terminal" | "logs" | "tests" | "actions";

const tabs: { id: BottomTab; label: string; icon: string }[] = [
  { id: "terminal", label: "Terminal", icon: ">" },
  { id: "logs", label: "Logs", icon: "📋" },
  { id: "tests", label: "Tests", icon: "🧪" },
  { id: "actions", label: "Actions", icon: "⚡" },
];

function TestsTab() {
  return (
    <div className="h-full bg-black p-2 overflow-auto font-mono text-xs">
      <div className="text-surface-text">Test Results</div>
      <div className="text-surface-muted mt-2 border border-surface-border rounded p-2">
        <div className="text-diff-add">✓ PASS auth/jwt.test.ts</div>
        <div className="text-diff-add">✓ PASS auth/login.test.ts</div>
        <div className="text-diff-remove">✕ FAIL auth/refresh.test.ts</div>
        <div className="text-surface-muted mt-1">
          ──────── Summary ────────
          <br />2 passed, 1 failed (3 total)
        </div>
      </div>
    </div>
  );
}

function ActionsTab() {
  const actions = [
    { time: "15:11:05", action: "Run npm test" },
    { time: "15:11:08", action: "Test failed: refresh.test.ts" },
    { time: "15:12:00", action: "Agent: Fix error in refresh.test.ts" },
  ];

  return (
    <div className="h-full bg-black p-2 overflow-auto">
      {actions.map((a, i) => (
        <div key={i} className="flex gap-3 py-0.5 font-mono text-xs">
          <span className="text-surface-muted flex-shrink-0">[{a.time}]</span>
          <span className="text-surface-text">{a.action}</span>
        </div>
      ))}
    </div>
  );
}

export default function BottomPanel() {
  const activeTab = useLayoutStore((s) => s.bottomTab);
  const setBottomTab = useLayoutStore((s) => s.setBottomTab);

  return (
    <div className="h-full flex flex-col border-t border-surface-border bg-black">
      {/* Tab 头部 */}
      <div className="flex items-center bg-surface-panel border-b border-surface-border no-select">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setBottomTab(tab.id)}
            className={`flex items-center gap-1 px-3 py-1.5 text-[11px] transition-colors ${
              activeTab === tab.id
                ? "text-surface-text border-t-2 border-t-accent-blue bg-surface-base"
                : "text-surface-muted hover:text-surface-text hover:bg-surface-border/20"
            }`}
          >
            <span className="text-[10px]">{tab.icon}</span>
            <span>{tab.label}</span>
          </button>
        ))}
        <div className="flex-1" />
      </div>

      {/* Tab 内容 */}
      <div className="flex-1 overflow-hidden">
        {activeTab === "terminal" && <Terminal />}
        {activeTab === "logs" && <LogView />}
        {activeTab === "tests" && <TestsTab />}
        {activeTab === "actions" && <ActionsTab />}
      </div>
    </div>
  );
}
