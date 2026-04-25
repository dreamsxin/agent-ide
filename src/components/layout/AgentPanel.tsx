import { useState } from "react";
import ChatView from "../agent/ChatView";
import TaskView from "../agent/TaskView";
import DiffView from "../agent/DiffView";
import AgentSelector from "../agent/AgentSelector";
import TaskPipeline from "../agent/TaskPipeline";
import SettingsPanel from "../agent/SettingsPanel";

type TabId = "chat" | "tasks" | "diff" | "pipeline" | "settings";

const tabs: { id: TabId; label: string; icon: string }[] = [
  { id: "chat", label: "Chat", icon: "💬" },
  { id: "tasks", label: "Tasks", icon: "📋" },
  { id: "diff", label: "Diff", icon: "🔄" },
  { id: "pipeline", label: "Pipe", icon: "⚙" },
  { id: "settings", label: "Set", icon: "🔧" },
];

export default function AgentPanel() {
  const [activeTab, setActiveTab] = useState<TabId>("chat");

  return (
    <div className="h-full flex flex-col bg-surface-panel border-l border-surface-border">
      {/* Tab 头部 — 紧凑布局 */}
      <div className="flex border-b border-surface-border no-select flex-shrink-0">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`flex items-center gap-1 px-2 py-1.5 text-[11px] transition-colors whitespace-nowrap ${
              activeTab === tab.id
                ? "text-surface-text border-b-2 border-accent-blue bg-surface-base/50"
                : "text-surface-muted hover:text-surface-text hover:bg-surface-border/20"
            }`}
          >
            <span className="text-xs">{tab.icon}</span>
            <span>{tab.label}</span>
          </button>
        ))}
        <div className="flex-1" />
      </div>

      {/* Tab 内容 */}
      <div className="flex-1 min-h-0 overflow-hidden">
        {activeTab === "chat" && <ChatView />}
        {activeTab === "tasks" && <TaskView />}
        {activeTab === "diff" && <DiffView />}
        {activeTab === "pipeline" && (
          <div className="flex flex-col h-full overflow-auto">
            <AgentSelector />
            <div className="border-t border-surface-border" />
            <div className="flex-1 overflow-auto">
              <TaskPipeline />
            </div>
          </div>
        )}
        {activeTab === "settings" && <SettingsPanel />}
      </div>
    </div>
  );
}
