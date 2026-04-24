import { useState } from "react";
import ChatView from "../agent/ChatView";
import TaskView from "../agent/TaskView";
import DiffView from "../agent/DiffView";
import AgentSelector from "../agent/AgentSelector";
import TaskPipeline from "../agent/TaskPipeline";

type TabId = "chat" | "tasks" | "diff" | "pipeline";

const tabs: { id: TabId; label: string; icon: string }[] = [
  { id: "chat", label: "Chat", icon: "💬" },
  { id: "tasks", label: "Tasks", icon: "📋" },
  { id: "diff", label: "Diff", icon: "🔄" },
  { id: "pipeline", label: "Pipeline", icon: "⚙" },
];

export default function AgentPanel() {
  const [activeTab, setActiveTab] = useState<TabId>("chat");

  return (
    <div className="h-full flex flex-col bg-surface-panel border-l border-surface-border">
      {/* Tab 头部 */}
      <div className="flex border-b border-surface-border no-select">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`flex items-center gap-1.5 px-3 py-2 text-xs transition-colors ${
              activeTab === tab.id
                ? "text-surface-text border-b-2 border-accent-blue bg-surface-base/50"
                : "text-surface-muted hover:text-surface-text hover:bg-surface-border/20"
            }`}
          >
            <span>{tab.icon}</span>
            <span>{tab.label}</span>
          </button>
        ))}
      </div>

      {/* Tab 内容 */}
      <div className="flex-1 overflow-hidden">
        {activeTab === "chat" && <ChatView />}
        {activeTab === "tasks" && <TaskView />}
        {activeTab === "diff" && <DiffView />}
        {activeTab === "pipeline" && (
          <div className="flex flex-col h-full overflow-auto">
            <AgentSelector />
            <div className="border-t border-surface-border" />
            <TaskPipeline />
          </div>
        )}
      </div>
    </div>
  );
}
