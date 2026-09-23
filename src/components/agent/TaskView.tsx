import { useEffect, useState } from "react";
import {
  Circle,
  CircleCheck,
  CircleSlash,
  CircleX,
  LoaderCircle,
  MoveDown,
  MoveUp,
  Play,
  RefreshCcw,
  SkipForward,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useAgentStore } from "../../stores/useAgentStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useT } from "../../i18n";
import type { Step } from "../../types/agent";

const statusConfig: Record<
  Step["status"],
  { icon: LucideIcon; color: string }
> = {
  todo: { icon: Circle, color: "text-surface-muted" },
  doing: { icon: LoaderCircle, color: "text-accent-blue" },
  done: { icon: CircleCheck, color: "text-diff-add" },
  error: { icon: CircleX, color: "text-diff-remove" },
  skipped: { icon: CircleSlash, color: "text-surface-muted" },
};

export default function TaskView({ embedded = false }: { embedded?: boolean }) {
  const t = useT();
  const steps = useAgentStore((s) => s.steps);
  const agentState = useAgentStore((s) => s.state);
  const currentTask = useAgentStore((s) => s.currentTask);
  const agentRunId = useAgentStore((s) => s.agentRunId);
  const restoredSession = useAgentStore((s) => s.restoredSession);
  const error = useAgentStore((s) => s.error);

  const chatProfileId = useAgentStore((s) => s.chatProfileId);
  const activeProfileId = useAgentStore((s) => s.activeProfileId);
  const chatContextCompression = useAgentStore((s) => s.chatContextCompression);
  const contextCompression = useAgentStore((s) => s.contextCompression);
  const updateAgentStep = useAgentStore((s) => s.updateAgentStep);
  const updateAgentSteps = useAgentStore((s) => s.updateAgentSteps);
  const skipAgentStep = useAgentStore((s) => s.skipAgentStep);
  const runAgentStep = useAgentStore((s) => s.runAgentStep);
  const startNewSession = useAgentStore((s) => s.startNewSession);
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const fileContents = useEditorStore((s) => s.fileContents);
  const selectedText = useEditorStore((s) => s.selectedText);

  // 没有任务时说"还没有"，不编一个像任务名的字面量：单步执行 / 续跑 / 修复都不设
  // 标题（它们接的是已有任务），而恢复出来的会话可能根本没有任务。
  const title = currentTask?.title ?? t("plan.noTaskYet");
  const canRun = agentState === "idle" || agentState === "done" || agentState === "waiting_user" || agentState === "error";

  const updateStepField = async (step: Step, updates: Partial<Step>) => {
    await updateAgentStep({ ...step, ...updates });
  };

  const moveStep = async (index: number, direction: -1 | 1) => {
    const target = index + direction;
    if (target < 0 || target >= steps.length) return;
    const next = [...steps];
    [next[index], next[target]] = [next[target], next[index]];
    await updateAgentSteps(next);
  };

  const runStep = async (step: Step, moreContext = false) => {
    await runAgentStep({
      step,
      activeFile: activeFile ?? undefined,
      activeFileContent: activeFile ? fileContents[activeFile] : undefined,
      selection: selectedText ?? undefined,
      contextFiles: openFiles.map((file) => file.path),
      profileId: chatProfileId ?? activeProfileId,
      contextCompression: chatContextCompression ?? contextCompression,
      contextSources: {
        includeGitDiff: moreContext || true,
        includeProjectTree: moreContext || true,
      },
      extraPrompt: moreContext
        ? "Regenerate this step with broader context. Include workspace tree, git diff, Problems, failed run output, terminal output, and recent warning/error logs when available."
        : undefined,
    });
  };

  return (
    <div className={`space-y-2 p-3 animate-fade-in ${embedded ? "" : "h-full overflow-auto"}`}>
      {/* 任务标题 + 状态 */}
      <div className="flex items-center justify-between mb-3">
        <span className="text-xs font-semibold text-surface-text">{title}</span>
        {/* 状态名以前直接渲染枚举值（`waiting_user` 这种），它是给代码看的，不是给人看的 */}
        <span className="text-[10px] text-surface-muted">{t(`state.${agentState}`)}</span>
      </div>
      {/*
        错误要在这个面板里也显示。原来只有 Chat 和 Changes 两个页签渲染 `error`，而它们和
        Plan 是互斥的页签 —— 也就是说在这里点 Clear 之后"前端清了、后端的对话还在"那条提示
        谁也看不见，界面看起来和成功一模一样。
      */}
      {error && (
        <div className="rounded border border-diff-delete/40 bg-diff-delete/10 px-2 py-1.5 text-[11px] text-diff-delete">
          {error}
        </div>
      )}
      {steps.length > 0 && restoredSession && (

        <div className="rounded border border-diff-modify/30 bg-diff-modify/10 px-2 py-1.5 text-[11px] text-surface-muted">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0 flex-1">
              <div className="text-surface-text">
                {restoredSession.interrupted
                  ? t("plan.restored.interrupted")
                  : t("plan.restored.normal")}
              </div>
              <div className="mt-0.5 truncate font-mono text-[10px]">
                {(agentRunId ?? restoredSession.runId) || t("plan.restored.noRunId")} ·{" "}
                {formatRestoreTime(restoredSession.restoredAt, t("plan.restored.fallback"))}
              </div>
              <div className={`mt-0.5 text-[10px] ${restoredSession.backendMatched ? "text-diff-add" : "text-diff-modify"}`}>
                {restoredSession.backendMatched === null
                  ? t("plan.restored.checking")
                  : restoredSession.backendMatched
                  ? t("plan.restored.matched")
                  : t("plan.restored.frontendOnly")}
              </div>
              <div className="mt-0.5">{t("plan.restored.continue")}</div>
            </div>
            <button
              onClick={() => {
                // 拒绝（运行还在跑）已经写进 store.error，下面那块错误条会显示它
                void startNewSession().catch(() => undefined);
              }}
              title={t("plan.newTask.title")}
              className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] hover:bg-surface-border/30"
            >
              {t("plan.newTask")}
            </button>

          </div>
        </div>
      )}

      {/* 步骤列表 */}
      {steps.length > 0 ? (
        steps.map((step, index) => {
          const config = statusConfig[step.status];
          const StatusIcon = config.icon;
          return (
            <div
              key={step.id}
              className={`space-y-2 px-2 py-2 rounded text-xs border border-transparent transition-colors ${
                step.status === "doing"
                  ? "bg-accent-blue/10 border-accent-blue/30"
                  : "hover:bg-surface-border/20"
              }`}
            >
              <div className="flex items-center gap-2">
                <StatusIcon
                  aria-hidden="true"
                  className={`h-3.5 w-3.5 flex-shrink-0 ${config.color} ${
                    step.status === "doing" ? "animate-spin" : ""
                  }`}
                />
                <EditableStepTitle
                  step={step}
                  label={t("plan.stepTitle")}
                  onCommit={(title) => updateStepField(step, { title })}
                />
              </div>
              <div className="grid grid-cols-2 gap-1">
                <select
                  value={step.scope ?? "workspace"}
                  onChange={(event) => void updateStepField(step, { scope: event.target.value as Step["scope"] })}
                  className="rounded border border-surface-border bg-surface-base px-1 py-0.5 text-[10px] text-surface-text"
                >
                  <option value="selection">{t("plan.scope.selection")}</option>
                  <option value="active_file">{t("plan.scope.activeFile")}</option>
                  <option value="open_files">{t("plan.scope.openFiles")}</option>
                  <option value="workspace">{t("plan.scope.workspace")}</option>
                </select>
                <select
                  value={step.executionMode ?? "diff"}
                  onChange={(event) => void updateStepField(step, { executionMode: event.target.value as Step["executionMode"] })}
                  className="rounded border border-surface-border bg-surface-base px-1 py-0.5 text-[10px] text-surface-text"
                >
                  <option value="analyze">{t("plan.exec.analyze")}</option>
                  <option value="diff">{t("plan.exec.diff")}</option>
                  <option value="test">{t("plan.exec.test")}</option>
                  <option value="fix">{t("plan.exec.fix")}</option>
                </select>
              </div>
              <div className="flex flex-wrap gap-1">
                <button
                  disabled={index === 0 || step.status === "doing"}
                  onClick={() => void moveStep(index, -1)}
                  className="inline-flex h-7 w-7 items-center justify-center rounded border border-surface-border text-surface-muted hover:bg-surface-border/30 disabled:cursor-not-allowed disabled:opacity-40"
                  title={t("plan.moveUp")}
                  aria-label={t("plan.moveUp")}
                >
                  <MoveUp aria-hidden="true" className="h-3.5 w-3.5" />
                </button>
                <button
                  disabled={index === steps.length - 1 || step.status === "doing"}
                  onClick={() => void moveStep(index, 1)}
                  className="inline-flex h-7 w-7 items-center justify-center rounded border border-surface-border text-surface-muted hover:bg-surface-border/30 disabled:cursor-not-allowed disabled:opacity-40"
                  title={t("plan.moveDown")}
                  aria-label={t("plan.moveDown")}
                >
                  <MoveDown aria-hidden="true" className="h-3.5 w-3.5" />
                </button>
                <button
                  disabled={!canRun || step.status === "doing"}
                  onClick={() => void runStep(step)}
                  className="inline-flex h-7 items-center gap-1 rounded border border-accent-blue/40 px-2 text-[10px] text-accent-blue hover:bg-accent-blue/10 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  <Play aria-hidden="true" className="h-3 w-3" />
                  {t("plan.run")}
                </button>
                <button
                  disabled={!canRun || step.status === "doing"}
                  onClick={() => void runStep(step, true)}
                  title={t("plan.retry.title")}
                  className="inline-flex h-7 items-center gap-1 rounded border border-surface-border px-2 text-[10px] text-surface-text hover:bg-surface-border/30 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  <RefreshCcw aria-hidden="true" className="h-3 w-3" />
                  {t("plan.retry")}
                </button>
                <button
                  disabled={step.status === "doing" || step.status === "skipped"}
                  onClick={() => void skipAgentStep(step.id)}
                  className="inline-flex h-7 w-7 items-center justify-center rounded border border-surface-border text-surface-muted hover:bg-surface-border/30 disabled:cursor-not-allowed disabled:opacity-40"
                  title={t("plan.skip")}
                  aria-label={t("plan.skip")}
                >
                  <SkipForward aria-hidden="true" className="h-3.5 w-3.5" />
                </button>
              </div>
            </div>
          );
        })
      ) : (
        <div className="text-xs text-surface-muted text-center py-6 space-y-2">
          <div>{t("plan.noActiveTask")}</div>
          <div className="text-[10px]">{t("plan.noActiveTask.hint")}</div>
        </div>
      )}

      {/* 步骤日志 */}
      {steps.some((s) => s.logs.length > 0) && (
        <div className="mt-4 border-t border-surface-border pt-3">
          <div className="text-[10px] text-surface-muted mb-2">{t("plan.stepLogs")}</div>
          {steps
            .filter((s) => s.logs.length > 0)
            .map((step) =>
              step.logs.map((log, i) => (
                <div
                  key={`${step.id}-${i}`}
                  className="text-[10px] text-surface-muted font-mono bg-surface-base rounded px-2 py-1 mb-1 whitespace-pre-wrap break-all"
                >
                  {log}
                </div>
              ))
            )}
        </div>
      )}
    </div>
  );
}

function EditableStepTitle({
  step,
  label,
  onCommit,
}: {
  step: Step;
  label: string;
  onCommit: (title: string) => Promise<void>;
}) {
  const [value, setValue] = useState(step.title);

  useEffect(() => setValue(step.title), [step.title]);

  const commit = () => {
    const next = value.trim();
    if (!next) {
      setValue(step.title);
      return;
    }
    if (next !== step.title) void onCommit(next);
  };

  return (
    <input
      value={value}
      onChange={(event) => setValue(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") event.currentTarget.blur();
        if (event.key === "Escape") {
          setValue(step.title);
          event.currentTarget.blur();
        }
      }}
      aria-label={label}
      className={`min-w-0 flex-1 rounded border border-transparent bg-transparent px-1 py-0.5 outline-none focus:border-accent-blue focus:bg-surface-base ${
        step.status === "done" || step.status === "skipped"
          ? "text-surface-muted"
          : "text-surface-text"
      }`}
    />
  );
}

function formatRestoreTime(value: number, fallback: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return fallback;
  return date.toLocaleTimeString();
}
