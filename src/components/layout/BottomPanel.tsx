import { useLayoutStore } from "../../stores/useLayoutStore";
import Terminal from "../panels/Terminal";
import LogView from "../panels/LogView";
import ProblemsPanel from "../panels/ProblemsPanel";
import TasksPanel from "../panels/TasksPanel";

type BottomTab = "terminal" | "commands" | "problems" | "logs";

const tabs: { id: BottomTab; label: string; icon: string; tooltip: string }[] = [
  { id: "terminal", label: "Terminal", icon: ">", tooltip: "Integrated system terminal" },
  { id: "commands", label: "Commands", icon: ">", tooltip: "Build, test, run, and debug project commands" },
  { id: "problems", label: "Problems", icon: "!", tooltip: "Diagnostics, test failures, and Agent findings" },
  { id: "logs", label: "Logs", icon: "📋", tooltip: "Agent & system operation logs" },
];

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
            title={tab.tooltip}
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
        <div className={activeTab === "terminal" ? "h-full" : "hidden h-full"}>
          <Terminal />
        </div>
        <div className={activeTab === "commands" ? "h-full" : "hidden h-full"}>
          <TasksPanel />
        </div>
        <div className={activeTab === "problems" ? "h-full" : "hidden h-full"}>
          <ProblemsPanel />
        </div>
        <div className={activeTab === "logs" ? "h-full" : "hidden h-full"}>
          <LogView />
        </div>
      </div>
    </div>
  );
}
