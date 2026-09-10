import { useMemo } from "react";
import { GitBranch } from "lucide-react";
import { useAgentStore } from "../../stores/useAgentStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useGitStore } from "../../stores/useGitStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useProblemStore, type ProblemSeverity } from "../../stores/useProblemStore";
import StatusDot from "../shared/StatusDot";

/** 严重级别的显示样式，与 ProblemsPanel / LSP 弹层里的 E/W/I 约定保持一致 */
const SEVERITY_COLOR: Record<ProblemSeverity, string> = {
  error: "text-diff-remove",
  warning: "text-diff-modify",
  info: "text-accent-blue",
};

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

  const counts = useMemo(
    () => ({
      error: problems.filter((problem) => problem.severity === "error").length,
      warning: problems.filter((problem) => problem.severity === "warning").length,
      info: problems.filter((problem) => problem.severity === "info").length,
    }),
    [problems]
  );

  const activeTab = openFiles.find((file) => file.path === activeFile) ?? null;

  // 点数字就该到得了列表，否则这个数字只是让人知道有问题却不知道去哪看
  const showProblems = () => {
    setBottomTab("problems");
    if (!bottomVisible) toggleBottomPanel();
  };

  const showSourceControl = () => {
    setLeftTab("git");
    if (!leftVisible) toggleLeftPanel();
  };

  const problemLabel =
    counts.error + counts.warning + counts.info === 0
      ? "No problems"
      : `${counts.error} error${counts.error === 1 ? "" : "s"}, ` +
        `${counts.warning} warning${counts.warning === 1 ? "" : "s"}, ` +
        `${counts.info} info`;

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
            title={`On branch ${gitStatus.branch}${
              gitStatus.upstream ? ` (tracking ${gitStatus.upstream})` : " (no upstream)"
            } — click to open Source Control`}
            className="flex items-center gap-1 rounded px-1 font-mono hover:bg-surface-border/40 hover:text-surface-text"
          >
            <GitBranch aria-hidden="true" className="h-3 w-3" />
            {gitStatus.branch}
            {gitStatus.entries.length > 0 && <span title="Uncommitted changes">*</span>}
            {gitStatus.ahead > 0 && <span>{`\u2191${gitStatus.ahead}`}</span>}
            {gitStatus.behind > 0 && <span>{`\u2193${gitStatus.behind}`}</span>}
          </button>
        )}

        <button
          type="button"
          onClick={showProblems}
          data-testid="status-bar-problems"
          aria-label={`${problemLabel}. Open the Problems panel.`}
          title={`${problemLabel} — click to open the Problems panel`}
          className="flex items-center gap-1.5 rounded px-1 hover:bg-surface-border/40 hover:text-surface-text"
        >
          <span className={SEVERITY_COLOR.error}>{counts.error}</span>
          <span className={SEVERITY_COLOR.warning}>{counts.warning}</span>
          <span className={SEVERITY_COLOR.info}>{counts.info}</span>
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
        <span
          className="flex items-center gap-1.5"
          title={
            llmConfigured
              ? "An LLM profile is configured"
              : "No API credentials — open the Agent panel's Settings view to add a profile"
          }
        >
          <span
            aria-hidden="true"
            className={`inline-block h-2 w-2 flex-shrink-0 rounded-full ${
              llmConfigured ? "bg-green-500" : "bg-red-500"
            }`}
          />
          {llmConfigured ? "LLM ready" : "LLM not configured"}
        </span>

        <StatusDot state={agentState} />
      </div>
    </div>
  );
}
