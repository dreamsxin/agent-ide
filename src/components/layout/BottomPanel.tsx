import { lazy, Suspense, useEffect, useState } from "react";
import { CircleAlert, ListTree, ScrollText, SquareTerminal } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useLayoutStore } from "../../stores/useLayoutStore";
import PanelLoading from "../shared/PanelLoading";

const Terminal = lazy(() => import("../panels/Terminal"));
const LogView = lazy(() => import("../panels/LogView"));
const ProblemsPanel = lazy(() => import("../panels/ProblemsPanel"));
const TasksPanel = lazy(() => import("../panels/TasksPanel"));

type BottomTab = "terminal" | "commands" | "problems" | "logs";

const tabs: { id: BottomTab; label: string; icon: LucideIcon; tooltip: string }[] = [
  { id: "terminal", label: "Terminal", icon: SquareTerminal, tooltip: "Integrated system terminal" },
  { id: "commands", label: "Commands", icon: ListTree, tooltip: "Build, test, run, and debug project commands" },
  { id: "problems", label: "Problems", icon: CircleAlert, tooltip: "Diagnostics, test failures, and Agent findings" },
  { id: "logs", label: "Logs", icon: ScrollText, tooltip: "Agent & system operation logs" },
];

export default function BottomPanel() {
  const activeTab = useLayoutStore((s) => s.bottomTab);
  const setBottomTab = useLayoutStore((s) => s.setBottomTab);
  const [visitedTabs, setVisitedTabs] = useState<Set<BottomTab>>(
    () => new Set([activeTab])
  );

  useEffect(() => {
    setVisitedTabs((current) => {
      if (current.has(activeTab)) return current;
      return new Set([...current, activeTab]);
    });
  }, [activeTab]);

  const openTab = (tab: BottomTab) => {
    setVisitedTabs((current) =>
      current.has(tab) ? current : new Set([...current, tab])
    );
    setBottomTab(tab);
  };

  return (
    <div data-testid="bottom-panel" className="h-full flex flex-col border-t border-surface-border bg-surface-base">
      {/* Tab 头部 */}
      <div className="flex items-center bg-surface-panel border-b border-surface-border no-select">
        {tabs.map((tab) => {
          const Icon = tab.icon;
          return (
            <button
              key={tab.id}
              onClick={() => openTab(tab.id)}
              title={tab.tooltip}
              data-testid={`bottom-tab-${tab.id}`}
              className={`flex items-center gap-1.5 px-3 py-1.5 text-[11px] transition-colors ${
                activeTab === tab.id
                  ? "text-surface-text border-t-2 border-t-accent-blue bg-surface-base"
                  : "text-surface-muted hover:text-surface-text hover:bg-surface-border/20"
              }`}
            >
              <Icon aria-hidden="true" className="h-3 w-3" />
              <span>{tab.label}</span>
            </button>
          );
        })}
        <div className="flex-1" />
      </div>

      {/* Tab 内容 */}
      <div className="flex-1 overflow-hidden">
        {visitedTabs.has("terminal") && (
          <div className={activeTab === "terminal" ? "h-full" : "hidden h-full"}>
            <Suspense fallback={<PanelLoading label="Loading terminal" />}>
              <Terminal />
            </Suspense>
          </div>
        )}
        {visitedTabs.has("commands") && (
          <div className={activeTab === "commands" ? "h-full" : "hidden h-full"}>
            <Suspense fallback={<PanelLoading label="Loading commands" />}>
              <TasksPanel />
            </Suspense>
          </div>
        )}
        {visitedTabs.has("problems") && (
          <div className={activeTab === "problems" ? "h-full" : "hidden h-full"}>
            <Suspense fallback={<PanelLoading label="Loading problems" />}>
              <ProblemsPanel />
            </Suspense>
          </div>
        )}
        {visitedTabs.has("logs") && (
          <div className={activeTab === "logs" ? "h-full" : "hidden h-full"}>
            <Suspense fallback={<PanelLoading label="Loading logs" />}>
              <LogView />
            </Suspense>
          </div>
        )}
      </div>
    </div>
  );
}
