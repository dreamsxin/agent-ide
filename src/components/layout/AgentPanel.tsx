import { lazy, Suspense, type ReactNode } from "react";
import {
  FileDiff,
  History,
  ListChecks,
  MessageSquare,
  Plus,
  Settings,
  Workflow,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import AgentRunSummary from "../agent/AgentRunSummary";
import ChatView from "../agent/ChatView";
import PanelLoading from "../shared/PanelLoading";
import { useAgentStore } from "../../stores/useAgentStore";
import { useLayoutStore, type AgentViewId } from "../../stores/useLayoutStore";
import { summarizeAgentRun } from "../../utils/agentExperience";
import { summarizeExternalActions } from "../../utils/externalActions";
import { changesBadge } from "./agentTabBadges";

const AgentSelector = lazy(() => import("../agent/AgentSelector"));
const DiffView = lazy(() => import("../agent/DiffView"));
const SessionHistory = lazy(() => import("../agent/SessionHistory"));
const SettingsPanel = lazy(() => import("../agent/SettingsPanel"));
const TaskPipeline = lazy(() => import("../agent/TaskPipeline"));
const TaskView = lazy(() => import("../agent/TaskView"));

type PrimaryViewId = Extract<AgentViewId, "task" | "plan" | "changes">;

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
  const activeView = useLayoutStore((store) => store.agentView);
  const setActiveView = useLayoutStore((store) => store.setAgentView);
  const steps = useAgentStore((store) => store.steps);
  const diffs = useAgentStore((store) => store.diffs);
  const externalActions = useAgentStore((store) => store.externalActions);
  const startNewSession = useAgentStore((store) => store.startNewSession);
  const summary = summarizeAgentRun(steps, diffs);
  const externalSummary = summarizeExternalActions(
    // 角标只数**这一次会话**的：恢复出来的历史一直在，没有任何操作能让它归零，而一个
    // 永远亮着又点不掉的角标会把这个提示训练成噪声。历史仍然在审查区里列着，带着
    // "previous session" 的标 —— 藏起来的是提醒，不是记录。
    externalActions.filter((action) => !action.restored)
  );
  const changes = changesBadge(summary.pendingChanges, externalSummary.performed);

  const badgeFor = (view: PrimaryViewId) => {
    if (view === "plan" && summary.totalSteps > 0) {
      return { text: String(summary.totalSteps), tone: "plan" as const, hint: "" };
    }
    if (view === "changes" && changes) return changes;
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
                    title={badge.hint || undefined}
                    className={`min-w-4 rounded px-1 py-0.5 text-center font-mono text-[9px] leading-none ${
                      badge.tone === "pending"
                        ? "bg-diff-modify/15 text-diff-modify"
                        : badge.tone === "external"
                          ? "bg-amber-500/15 text-amber-300"
                          : "bg-surface-border/70 text-surface-muted"
                    }`}
                  >
                    {badge.text}
                  </span>
                )}
                {active && <span className="absolute inset-x-1 bottom-0 h-0.5 bg-accent-blue" />}
              </button>
            );
          })}
        </div>

        <div className="my-2 w-px bg-surface-border" />

        {/* 新建会话 / 历史会话之前在界面上没有任何入口，只有恢复横幅里那个按钮沾边，
            而它要等"有恢复出来的步骤"才出现。放在这里是因为它们是会话级操作，和下面
            那两个配置入口同一层。 */}
        <UtilityButton
          active={false}
          icon={Plus}
          label="New session"
          onClick={() => {
            // 被拒绝时错误已经进 store.error，Task / Chat 视图上的错误条会显示
            void startNewSession().catch(() => undefined);
          }}
          testId="agent-tab-new-session"
        />
        <UtilityButton
          active={activeView === "sessions"}
          icon={History}
          label="Session history"
          onClick={() => setActiveView("sessions")}
          testId="agent-tab-sessions"
        />
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
          <Suspense fallback={<PanelLoading label="Loading task plan" />}>
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
          </Suspense>
        )}
        {activeView === "changes" && (
          <Suspense fallback={<PanelLoading label="Loading changes" />}>
            <PrimaryView
              onOpenChanges={() => setActiveView("changes")}
              onOpenPlan={() => setActiveView("plan")}
            >
              <DiffView />
            </PrimaryView>
          </Suspense>
        )}
        {activeView === "sessions" && (
          <Suspense fallback={<PanelLoading label="Loading sessions" />}>
            <SessionHistory />
          </Suspense>
        )}
        {activeView === "pipeline" && (
          <Suspense fallback={<PanelLoading label="Loading pipeline" />}>
            <div className="flex h-full flex-col overflow-auto">
              <AgentSelector />
              <div className="border-t border-surface-border" />
              <div className="flex-1 overflow-auto">
                <TaskPipeline />
              </div>
            </div>
          </Suspense>
        )}
        {activeView === "settings" && (
          <Suspense fallback={<PanelLoading label="Loading settings" />}>
            <SettingsPanel />
          </Suspense>
        )}
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
