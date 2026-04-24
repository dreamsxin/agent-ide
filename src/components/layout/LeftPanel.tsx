import Explorer from "../panels/Explorer";
import GitPanel from "../panels/GitPanel";
import { useLayoutStore } from "../../stores/useLayoutStore";

const tabs: { id: "explorer" | "git"; label: string; icon: string }[] = [
  { id: "explorer", label: "Files", icon: "📁" },
  { id: "git", label: "Git", icon: "⬢" },
];

export default function LeftPanel() {
  const leftTab = useLayoutStore((s) => s.leftTab);
  const setLeftTab = useLayoutStore((s) => s.setLeftTab);

  return (
    <div className="h-full flex flex-col border-r border-surface-border bg-surface-panel">
      {/* Tab 头部 */}
      <div className="flex border-b border-surface-border no-select">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setLeftTab(tab.id)}
            className={`flex items-center gap-1.5 px-3 py-1.5 text-[11px] transition-colors ${
              leftTab === tab.id
                ? "text-surface-text border-b-2 border-b-accent-blue bg-surface-base/50"
                : "text-surface-muted hover:text-surface-text hover:bg-surface-border/20"
            }`}
          >
            <span className="text-[10px]">{tab.icon}</span>
            <span>{tab.label}</span>
          </button>
        ))}
      </div>

      {/* Tab 内容 */}
      <div className="flex-1 overflow-hidden">
        {leftTab === "explorer" && <Explorer />}
        {leftTab === "git" && <GitPanel />}
      </div>
    </div>
  );
}
