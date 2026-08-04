import { useState, type ReactNode } from "react";
import {
  FileDiff,
  ListChecks,
  MessageSquare,
  Settings,
  Workflow,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import AgentRunSummary from "../agent/AgentRunSummary";
import AgentSelector from "../agent/AgentSelector";
import ChatView from "../agent/ChatView";
import DiffView from "../agent/DiffView";
import SettingsPanel from "../agent/SettingsPanel";
import TaskPipeline from "../agent/TaskPipeline";
import TaskView from "../agent/TaskView";
import { useAgentStore } from "../../stores/useAgentStore";
import { summarizeAgentRun } from "../../utils/agentExperience";

type ViewId = "task" | "plan" | "changes" | "pipeline" | "settings";
type PrimaryViewId = Extract<ViewId, "task" | "plan" | "changes">;

const primaryViews: Array<{
  id: PrimaryViewId;
  label: string;
  icon: LucideIcon;
  testId: string;
}> = [
  { id: "task", label: "Task", icon: MessageSquare, testId: "agent-tab-chat" },
  { id: "plan", label: "Plan", icon: ListChecks, testId: "agent-tab-tasks" },
  { id: "changes", label: "Changes", icon: FileDiff, testId: "agent-tab-diff" },
];

export default function AgentPanel() {
  const [activeView, setActiveView] = useState<ViewId>("task");
  const steps = useAgentStore((store) => store.steps);
  const diffs = useAgentStore((store) => store.diffs);
  const summary = summarizeAgentRun(steps, diffs);

  const badgeFor = (view: PrimaryViewId) => {
    if (view === "plan" && summary.totalSteps > 0) return summary.totalSteps;
    if (view === "changes" && summary.pendingChanges > 0) return summary.pendingChanges;
    return null;
  };

  return (
    <div
      data-testid="agent-panel"
      className="flex h-full flex-col border-l border-surface-border bg-surface-panel"
    >
      <nav
        className="flex h-9 flex-shrink-0 items-stretch border-b border-surface-border px-1 no-select"
        aria-label="Agent task views"
      >
        <div className="flex min-w-0 flex-1 items-stretch">
          {primaryViews.map((view) => {
            const Icon = view.icon;
            const badge = badgeFor(view.id);
            const active = activeView === view.id;
            return (
              <button
                key={view.id}
                type="button"
                onClick={() => setActiveView(view.id)}
                aria-pressed={active}
                title={`${view.label} view`}
                data-testid={view.testId}
                className={`relative flex min-w-0 items-center gap-1.5 px-2 text-[11px] transition-colors ${
                  active
                    ? "text-surface-text"
                    : "text-surface-muted hover:bg-surface-border/20 hover:text-surface-text"
                }`}
              >
                <Icon aria-hidden="true" className="h-3.5 w-3.5 flex-shrink-0" />
                <span className="truncate">{view.label}</span>
                {badge !== null && (
                  <span
                    className={`min-w-4 rounded px-1 py-0.5 text-center font-mono text-[9px] leading-none ${
                      view.id === "changes"
                        ? "bg-diff-modify/15 text-diff-modify"
                        : "bg-surface-border/70 text-surface-muted"
                    }`}
                  >
                    {badge}
                  </span>
                )}
                {active && <span className="absolute inset-x-1 bottom-0 h-0.5 bg-accent-blue" />}
              </button>
            );
          })}
        </div>

        <div className="my-2 w-px bg-surface-border" />

        <UtilityButton
          active={activeView === "pipeline"}
          icon={Workflow}
          label="Pipeline configuration"
          onClick={() => setActiveView("pipeline")}
          testId="agent-tab-pipeline"
        />
        <UtilityButton
          active={activeView === "settings"}
          icon={Settings}
          label="Agent settings"
          onClick={() => setActiveView("settings")}
          testId="agent-tab-settings"
        />
      </nav>

      <div className="min-h-0 flex-1 overflow-hidden">
        {activeView === "task" && (
          <PrimaryView
            onOpenChanges={() => setActiveView("changes")}
            onOpenPlan={() => setActiveView("plan")}
          >
            <ChatView />
          </PrimaryView>
        )}
        {activeView === "plan" && (
          <PrimaryView
            onOpenChanges={() => setActiveView("changes")}
            onOpenPlan={() => setActiveView("plan")}
          >
            <div className="h-full overflow-auto">
              <TaskView embedded />
              <div className="border-t border-surface-border">
                <TaskPipeline />
              </div>
            </div>
          </PrimaryView>
        )}
        {activeView === "changes" && (
          <PrimaryView
            onOpenChanges={() => setActiveView("changes")}
            onOpenPlan={() => setActiveView("plan")}
          >
            <DiffView />
          </PrimaryView>
        )}
        {activeView === "pipeline" && (
          <div className="flex h-full flex-col overflow-auto">
            <AgentSelector />
            <div className="border-t border-surface-border" />
            <div className="flex-1 overflow-auto">
              <TaskPipeline />
            </div>
          </div>
        )}
        {activeView === "settings" && <SettingsPanel />}
      </div>
    </div>
  );
}

function PrimaryView({
  children,
  onOpenChanges,
  onOpenPlan,
}: {
  children: ReactNode;
  onOpenChanges: () => void;
  onOpenPlan: () => void;
}) {
  return (
    <div className="flex h-full flex-col">
      <AgentRunSummary onOpenChanges={onOpenChanges} onOpenPlan={onOpenPlan} />
      <div className="min-h-0 flex-1">{children}</div>
    </div>
  );
}

function UtilityButton({
  active,
  icon: Icon,
  label,
  onClick,
  testId,
}: {
  active: boolean;
  icon: LucideIcon;
  label: string;
  onClick: () => void;
  testId: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      aria-pressed={active}
      title={label}
      data-testid={testId}
      className={`flex w-8 flex-shrink-0 items-center justify-center transition-colors ${
        active
          ? "text-accent-blue"
          : "text-surface-muted hover:bg-surface-border/20 hover:text-surface-text"
      }`}
    >
      <Icon aria-hidden="true" className="h-3.5 w-3.5" />
    </button>
  );
}
