import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTaskStore } from "../../stores/useTaskStore";
import { useAgentStore } from "../../stores/useAgentStore";
import { isTauriRuntime } from "../../utils/tauri";
import { useProjectTasks } from "../../hooks/useProjectTasks";
import { useRunProjectTask } from "../../hooks/useRunProjectTask";
import { useFixWithAgent } from "../../hooks/useFixWithAgent";
import { useT } from "../../i18n";
import {
  backendStatus,
  repairStatus,
  verificationStatus,
  type RepairWorkspaceReport,
  type StatusLine,
  type VerificationReport,
} from "./taskVerification";


export default function TasksPanel() {
  const t = useT();
  const lastTask = useTaskStore((s) => s.lastTask);
  const taskRuns = useTaskStore((s) => s.taskRuns);
  const taskRunHistory = useTaskStore((s) => s.taskRunHistory);
  const clearTaskRunHistory = useTaskStore((s) => s.clearTaskRunHistory);
  const { tasks, usingFallback, loading, error } = useProjectTasks();
  const runProjectTask = useRunProjectTask();
  const { fixTaskFailure, sendFixPrompt, isAgentBusy } = useFixWithAgent();
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [verifying, setVerifying] = useState(false);
  const [repairing, setRepairing] = useState(false);
  // 存键而不是句子：切语言时这一行要跟着变，存成字符串就会留着上一种语言
  const [verifyStatus, setVerifyStatus] = useState<StatusLine | null>(null);

  const selectedRun = useMemo(
    () => taskRunHistory.find((run) => run.runId === selectedRunId) ?? taskRunHistory[0],
    [selectedRunId, taskRunHistory]
  );

  const rerunHistoryEntry = (run: typeof selectedRun) => {
    if (!run) return;
    const task = tasks.find((item) => item.id === run.taskId) ?? {
      id: run.taskId,
      label: run.label,
      command: run.command,
      description: t("tasks.history.description"),
      source: "history",
    };
    void runProjectTask(task);
  };

  // 跑一遍全部检查；失败就把后端生成的修复提示直接发给 Agent。
  // 后端会挡掉 dev/watch 这类长驻命令并回报跳过了哪些。
  const verifyAll = async () => {
    if (!isTauriRuntime() || tasks.length === 0) return;
    setVerifying(true);
    setVerifyStatus(null);
    try {
      const report = await invoke<VerificationReport>("verify_workspace", {
        request: { commands: tasks.map((item) => item.command) },
      });
      setVerifyStatus(verificationStatus(report));
      if (report.repairPrompt) {
        await sendFixPrompt(report.repairPrompt);
      }
    } catch (error) {
      setVerifyStatus(backendStatus(error));
    } finally {
      setVerifying(false);
    }
  };


  // 有界自动修复：跑检查 → 失败让 Agent 改 → 落盘 → 再跑，直到通过或预算用完。
  // 和 Verify All 的区别不只是轮数：这个会自己把修改写进工作区，所以后端只在
  // Auto 模式下允许，别的模式直接报错——那条错误照原样显示，不在前端悄悄兜住。
  const repairAll = async () => {
    if (!isTauriRuntime() || tasks.length === 0) return;
    setRepairing(true);
    setVerifyStatus(null);
    try {
      const report = await invoke<RepairWorkspaceReport>("repair_workspace", {
        request: {
          commands: tasks.map((item) => item.command),
          maxIterations: 2,
          // 跟着聊天里选的 profile 和模型跑：不传的话后端退回当前活跃 profile，于是修复
          // 循环悄悄换到另一个模型上，价格、上限、窗口都变了而这里看不出来
          profileId: useAgentStore.getState().chatProfileId,
          modelOverride: useAgentStore.getState().chatModelOverride,
        },
      });
      setVerifyStatus(repairStatus(report));
    } catch (error) {
      setVerifyStatus(backendStatus(error));
    } finally {
      setRepairing(false);
    }
  };

  return (
    <div data-testid="commands-panel" className="flex h-full flex-col bg-black text-xs">

      <div className="flex items-center justify-between gap-3 border-b border-surface-border px-3 py-1.5">
        <div className="min-w-0">
          <div className="font-semibold text-surface-text">{t("tasks.title")}</div>
          <div className="truncate text-[11px] text-surface-muted">
            {t("tasks.discovered", { count: tasks.length })} ·{" "}
            {usingFallback ? t("tasks.source.fallback") : t("tasks.source.workspace")}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => void verifyAll()}
            disabled={!isTauriRuntime() || verifying || repairing || isAgentBusy || tasks.length === 0}
            data-testid="verify-all"

            title={t("tasks.verify.title")}
            className="rounded border border-accent-blue/40 px-1.5 py-0.5 text-[10px] text-accent-blue hover:bg-accent-blue/10 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {verifying ? t("tasks.verifying") : t("tasks.verify")}
          </button>
          <button
            onClick={() => void repairAll()}
            disabled={!isTauriRuntime() || verifying || repairing || isAgentBusy || tasks.length === 0}
            data-testid="repair-all"
            title={t("tasks.repair.title")}
            className="rounded border border-accent-blue/40 px-1.5 py-0.5 text-[10px] text-accent-blue hover:bg-accent-blue/10 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {repairing ? t("tasks.repairing") : t("tasks.repair")}
          </button>

          <div className="max-w-[280px] truncate text-[11px] text-surface-muted">
            {usingFallback ? t("tasks.hint.fallback") : t("tasks.hint.workspace")}
          </div>
        </div>
      </div>

      {/* verifyStatus 会装完整的后端错误（含 "requires Auto mode" 那类拒绝理由）。
          以前它和上面那句提示共用一个 `max-w-[280px] truncate`，几个词就被截断，
          而截断掉的正是原因。 */}
      {verifyStatus && (
        <div className="border-b border-surface-border px-2 py-1.5 text-[11px] leading-relaxed text-surface-muted whitespace-pre-wrap break-words">
          {verifyStatus.kind === "raw" ? verifyStatus.text : t(verifyStatus.key, verifyStatus.params)}
        </div>
      )}

      {!isTauriRuntime() && (
        <div className="border-b border-surface-border px-3 py-2 text-[11px] text-diff-modify">
          {t("tasks.needsTauri")}
        </div>
      )}

      {error && (
        <div className="border-b border-surface-border px-3 py-2 text-[11px] text-diff-remove">
          {t("tasks.discoverFailed", { error })}
        </div>
      )}

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(260px,0.38fr)_minmax(360px,1fr)]">
        <div className="min-w-0 border-r border-surface-border">
          <div className="grid grid-cols-[minmax(120px,0.9fr)_minmax(180px,1.3fr)_72px] border-b border-surface-border bg-surface-panel/70 px-3 py-1 text-[10px] uppercase text-surface-muted">
            <span>{t("tasks.col.command")}</span>
            <span>{t("tasks.col.script")}</span>
            <span className="text-right">{t("tasks.col.status")}</span>
          </div>
          <div className="h-full overflow-auto">
            {loading && (
              <div className="px-3 py-4 text-center text-[11px] text-surface-muted">
                {t("tasks.loading")}
              </div>
            )}
            {tasks.map((task) => {
              const runState = taskRuns[task.id];

  return (
                <button
                  key={task.id}
                  onClick={() => void runProjectTask(task)}
                  disabled={!isTauriRuntime()}
                  data-testid={`command-${sanitizeTestId(task.label)}`}
                  className="grid w-full grid-cols-[minmax(120px,0.9fr)_minmax(180px,1.3fr)_72px] items-center gap-2 border-b border-surface-border/40 px-3 py-1.5 text-left hover:bg-surface-border/20 disabled:cursor-not-allowed disabled:opacity-50"
                  title={task.description}
                >
                  <span className="min-w-0 truncate font-semibold text-surface-text">
                    {task.label}
                  </span>
                  <span className="min-w-0 truncate font-mono text-[11px] text-accent-blue">
                    {task.command}
                  </span>
                  <span className="text-right">
                    {/* 没跑过就显示来源。来源是 `package.json` 这类文件名，不翻：
                        翻过去用户就对不上自己项目里的那个文件了。 */}
                    <span className={statusClass(runState?.status ?? task.source)}>
                      {runState ? t(`tasks.status.${runState.status}`) : task.source}
                    </span>
                    {runState?.status === "failed" && (
                      <button
                        onClick={(event) => {
                          event.stopPropagation();
                          void fixTaskFailure(runState);
                        }}
                        disabled={isAgentBusy}
                        data-testid="fix-with-agent"
                        className="ml-1 rounded border border-accent-blue/40 px-1 py-0.5 text-[10px] text-accent-blue hover:bg-accent-blue/10 disabled:cursor-not-allowed disabled:opacity-40"
                      >
                        {t("tasks.fix")}
                      </button>
                    )}
                  </span>
                </button>
              );
            })}
          </div>
        </div>

        <div className="grid min-w-0 grid-rows-[minmax(96px,0.38fr)_minmax(120px,1fr)]">
          <div className="min-h-0 border-b border-surface-border">
            <div className="flex items-center justify-between gap-2 border-b border-surface-border px-3 py-1.5">
              <span data-testid="run-history" className="font-semibold text-surface-text">{t("tasks.history")}</span>
              {taskRunHistory.length > 0 && (
                <button
                  onClick={clearTaskRunHistory}
                  className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-muted hover:text-surface-text disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {t("tasks.history.clear")}
                </button>
              )}
            </div>
            <div className="h-full overflow-auto">
              {taskRunHistory.length === 0 ? (
                <div className="px-3 py-4 text-center text-[11px] text-surface-muted">
                  {t("tasks.history.empty")}
                </div>
              ) : (
                taskRunHistory.map((run) => (
                  <button
                    key={run.runId}
                    onClick={() => setSelectedRunId(run.runId)}
                    data-testid={`run-history-entry-${sanitizeTestId(run.label)}`}
                    className={`grid w-full grid-cols-[1fr_auto_auto] items-center gap-2 border-b border-surface-border/40 px-3 py-1.5 text-left ${
                      selectedRun?.runId === run.runId
                        ? "bg-accent-blue/10"
                        : "hover:bg-surface-border/20"
                    }`}
                  >
                    <span className="min-w-0 truncate font-semibold text-surface-text">
                      {run.label}
                    </span>
                    <span className={statusClass(run.status)}>
                      {t(`tasks.status.${run.status}`)}
                    </span>
                    <span className="font-mono text-[10px] text-surface-muted">
                      {formatDuration(run.durationMs)}
                    </span>
                  </button>
                ))
              )}
            </div>
          </div>

          <div className="min-h-0">
            <div className="flex items-center gap-2 border-b border-surface-border px-3 py-1.5">
              <span className="min-w-0 flex-1 truncate font-semibold text-surface-text">
                {selectedRun ? selectedRun.label : t("tasks.output")}
              </span>
                  {selectedRun && (
                <>
                  <span className="font-mono text-[10px] text-surface-muted">
                    {t("tasks.exit", {
                      code: selectedRun.exitCode ?? t("tasks.exit.unknown"),
                    })}
                  </span>
                  <button
                    onClick={() => rerunHistoryEntry(selectedRun)}
                    disabled={!isTauriRuntime()}
                    data-testid="run-history-rerun"
                    className="rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-muted hover:text-surface-text disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    {t("tasks.rerun")}
                  </button>
                  {selectedRun.status === "failed" && (
                    <button
                      onClick={() => void fixTaskFailure(selectedRun)}
                      disabled={isAgentBusy}
                      data-testid="run-history-fix-with-agent"
                      className="rounded border border-accent-blue/40 px-1.5 py-0.5 text-[10px] text-accent-blue hover:bg-accent-blue/10 disabled:cursor-not-allowed disabled:opacity-40"
                    >
                      {t("tasks.fixWithAgent")}
                    </button>
                  )}
                </>
              )}
            </div>
            <pre className="h-full overflow-auto whitespace-pre-wrap px-3 py-2 font-mono text-[10px] leading-relaxed text-surface-text">
              {selectedRun?.output?.trim() || t("tasks.output.empty")}
            </pre>
          </div>
        </div>
      </div>

      {lastTask && (
        <div className="border-t border-surface-border px-3 py-2 text-[11px] text-surface-muted">
          {t("tasks.lastQueued")}{" "}
          <span className="font-mono text-surface-text">{lastTask.command}</span>
        </div>
      )}
    </div>
  );
}

function sanitizeTestId(value: string) {
  return value.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

function statusClass(status: string) {
  if (status === "success") return "text-[10px] uppercase text-diff-add";
  if (status === "failed") return "text-[10px] uppercase text-diff-remove";
  if (status === "running") return "text-[10px] uppercase text-accent-blue";
  return "text-[10px] uppercase text-surface-muted";
}

function formatDuration(durationMs?: number) {
  if (durationMs === undefined) return "-";
  if (durationMs < 1000) return `${durationMs} ms`;
  return `${(durationMs / 1000).toFixed(1)} s`;
}
