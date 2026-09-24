import { useMemo } from "react";
import { GitBranch } from "lucide-react";
import { useAgentStore } from "../../stores/useAgentStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useGitStore } from "../../stores/useGitStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useProblemStore, type ProblemSeverity } from "../../stores/useProblemStore";
import { llmIndicator, llmTargetFingerprint } from "../../stores/llmConnection";
import { describeRunUsage } from "../../types/agent";
import { formatMicrosUsd } from "../../utils/money";
import StatusDot from "../shared/StatusDot";
import { useT } from "../../i18n";

/**
 * 每个严重级别的字母和颜色。
 *
 * 字母不是装饰：只用颜色区分三个数字，色觉障碍的用户看到的就是三个裸整数，
 * 分不清哪个是错误。`aria-label` 解决读屏，解决不了这个。
 */
const SEVERITY_STYLE: Record<ProblemSeverity, { letter: string; color: string }> = {
  error: { letter: "E", color: "text-diff-remove" },
  warning: { letter: "W", color: "text-diff-modify" },
  info: { letter: "I", color: "text-accent-blue" },
};

/** 连通性三档对应的点色；判断在 `stores/llmConnection.ts`，这里只是配色 */
const LLM_TONE_COLOR = {
  ok: "bg-green-500",
  warn: "bg-amber-500",
  error: "bg-red-500",
} as const;

/**
 * 底部状态栏：被动状态的归处。
 *
 * 这些信息原本挤在顶栏里，和 21 个可点控件混在一条 40px 的行上，其中两项还是
 * **没有文字的圆点** —— 用户得把鼠标停上去才知道那是 LLM 有没有配好。诊断数量
 * 更糟：只有打开底部面板的 Problems 页才看得到，于是"代码有几个错误"这种应该
 * 一直在视野里的事实，需要主动去翻。
 *
 * 这里只放**已经存在的数据**。编码和行尾在代码库里根本没建模，每次运行的花费
 * 也没有 store，所以不占位 —— 空着的段位比没有更糟。
 */
export default function StatusBar() {
  const problems = useProblemStore((s) => s.problems);
  const agentState = useAgentStore((s) => s.state);
  const llmConfigured = useAgentStore((s) => s.llmConfigured);
  const llmConnection = useAgentStore((s) => s.llmConnection);
  // 指纹在渲染时算，和存下来的那个比：这样"上一次测的还是不是现在这个目标"不依赖
  // 任何一处改配置的代码记得作废旧结果
  const llmTarget = useAgentStore(llmTargetFingerprint);
  const runUsage = useAgentStore((s) => s.runUsage);
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const cursorPosition = useEditorStore((s) => s.cursorPosition);
  const gitStatus = useGitStore((s) => s.status);
  const bottomVisible = useLayoutStore((s) => s.bottomVisible);
  const toggleBottomPanel = useLayoutStore((s) => s.toggleBottomPanel);
  const setBottomTab = useLayoutStore((s) => s.setBottomTab);
  const leftVisible = useLayoutStore((s) => s.leftVisible);
  const toggleLeftPanel = useLayoutStore((s) => s.toggleLeftPanel);
  const setLeftTab = useLayoutStore((s) => s.setLeftTab);
  const t = useT();

  const counts = useMemo(
    () => ({
      error: problems.filter((problem) => problem.severity === "error").length,
      warning: problems.filter((problem) => problem.severity === "warning").length,
      info: problems.filter((problem) => problem.severity === "info").length,
    }),
    [problems]
  );

  const activeTab = openFiles.find((file) => file.path === activeFile) ?? null;
  const usageDisplay = runUsage ? describeRunUsage(runUsage, formatMicrosUsd) : null;
  const llm = llmIndicator(llmConfigured, llmConnection, llmTarget);

  // 点数字就该到得了列表，否则这个数字只是让人知道有问题却不知道去哪看
  const showProblems = () => {
    setBottomTab("problems");
    if (!bottomVisible) toggleBottomPanel();
  };

  const showSourceControl = () => {
    setLeftTab("git");
    if (!leftVisible) toggleLeftPanel();
  };

  // 一句话，参数是三个数：英文是 "N error(s), …"，中文是"N 个错误，…"。
  // 以前这里按 `count === 1` 现拼单复数，那种拼法只有英文成立。
  const problemLabel =
    counts.error + counts.warning + counts.info === 0
      ? t("status.noProblems")
      : t("status.problems", {
          errors: counts.error,
          warnings: counts.warning,
          info: counts.info,
        });

  return (
    <div
      data-testid="status-bar"
      className="flex h-6 flex-shrink-0 items-center justify-between border-t border-surface-border bg-surface-base px-2 text-[10px] text-surface-muted"
    >
      <div className="flex items-center gap-3">
        {gitStatus && (
          <button
            type="button"
            onClick={showSourceControl}
            data-testid="status-bar-branch"
            aria-label={t("status.branch.aria", { branch: gitStatus.branch })}
            title={t("status.branch.title", {
              branch: gitStatus.branch,
              tracking: gitStatus.upstream
                ? t("status.branch.tracking", { upstream: gitStatus.upstream })
                : t("status.branch.noUpstream"),
            })}
            className="flex items-center gap-1 rounded px-1 font-mono hover:bg-surface-border/40 hover:text-surface-text"
          >
            <GitBranch aria-hidden="true" className="h-3 w-3" />
            {gitStatus.branch}
            {gitStatus.entries.length > 0 && <span title={t("status.uncommitted")}>*</span>}
            {gitStatus.ahead > 0 && <span>{`\u2191${gitStatus.ahead}`}</span>}
            {gitStatus.behind > 0 && <span>{`\u2193${gitStatus.behind}`}</span>}
          </button>
        )}

        <button
          type="button"
          onClick={showProblems}
          data-testid="status-bar-problems"
          aria-label={t("status.problems.aria", { summary: problemLabel })}
          title={t("status.problems.title", { summary: problemLabel })}
          className="flex items-center gap-1.5 rounded px-1 hover:bg-surface-border/40 hover:text-surface-text"
        >
          {(["error", "warning", "info"] as ProblemSeverity[]).map((severity) => (
            <span key={severity} className={SEVERITY_STYLE[severity].color}>
              {SEVERITY_STYLE[severity].letter}
              {counts[severity]}
            </span>
          ))}
        </button>

        {activeTab && (
          <>
            {cursorPosition && (
              <span data-testid="status-bar-cursor" className="font-mono">
                Ln {cursorPosition.line}, Col {cursorPosition.column}
              </span>
            )}
            <span title={activeTab.path} className="font-mono">
              {activeTab.language}
            </span>
          </>
        )}
      </div>

      <div className="flex items-center gap-3">
        {usageDisplay && (
          <span data-testid="status-bar-usage" title={usageDisplay.detail} className="font-mono">
            {usageDisplay.label}
          </span>
        )}

        <span
          data-testid="status-bar-llm"
          className="flex items-center gap-1.5"
          title={llm.title}
        >
          <span
            aria-hidden="true"
            className={`inline-block h-2 w-2 flex-shrink-0 rounded-full ${LLM_TONE_COLOR[llm.tone]}`}
          />
          {llm.label}
        </span>

        <StatusDot state={agentState} />
      </div>
    </div>
  );
}
