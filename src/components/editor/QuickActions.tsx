import { useMemo } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useAgentStore } from "../../stores/useAgentStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useMonacoContext } from "./MonacoContext";

const actions = [
  { key: "explain", label: "Explain", icon: "💡" },
  { key: "fix", label: "Fix", icon: "🔧" },
  { key: "refactor", label: "Refactor", icon: "♻️" },
  { key: "optimize", label: "Optimize", icon: "⚡" },
] as const;

type QuickActionKey = (typeof actions)[number]["key"];

const ACTION_PROMPTS: Record<QuickActionKey, string> = {
  explain: "Explain the selected code. Focus on behavior, inputs, outputs, side effects, and any hidden assumptions.",
  fix: "Find and fix bugs in the selected code. Return proposed code changes as reviewable diffs when a code change is needed.",
  refactor: "Refactor the selected code for readability and maintainability without changing behavior. Return proposed code changes as reviewable diffs.",
  optimize: "Optimize the selected code only where there is a clear performance or complexity benefit. Explain the tradeoff and return reviewable diffs if changing code.",
};

/**
 * 选区浮动工具栏 —— 选中文本时在选区上方弹出 [Explain | Fix | Refactor | Optimize]
 */
export default function QuickActions() {
  const selectedText = useEditorStore((s) => s.selectedText);
  const selectedRange = useEditorStore((s) => s.selectedRange);
  const activeFile = useEditorStore((s) => s.activeFile);
  const fileContents = useEditorStore((s) => s.fileContents);
  const sendPrompt = useAgentStore((s) => s.sendPrompt);
  const addMessage = useAgentStore((s) => s.addMessage);
  const agentState = useAgentStore((s) => s.state);
  const rightVisible = useLayoutStore((s) => s.rightVisible);
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel);
  const { editor, monaco } = useMonacoContext();

  // 计算工具栏的像素位置
  const position = useMemo(() => {
    if (!editor || !monaco || !selectedRange || !selectedText) return null;

    try {
      // 选区起始行的像素 Top
      const top = editor.getTopForLineNumber(selectedRange.startLine);
      const editorLayout = editor.getLayoutInfo();

      return {
        top: top - 40, // 选区上方 40px
        left: editorLayout.contentLeft + editorLayout.contentWidth / 2,
      };
    } catch {
      return null;
    }
  }, [selectedText, selectedRange, editor, monaco]);

  const isAgentBusy =
    agentState !== "idle" &&
    agentState !== "done" &&
    agentState !== "error" &&
    agentState !== "waiting_user";

  const runQuickAction = async (action: QuickActionKey) => {
    if (!selectedText || isAgentBusy) return;
    if (!rightVisible) {
      toggleRightPanel();
    }

    const rangeText = selectedRange
      ? `lines ${selectedRange.startLine}-${selectedRange.endLine}`
      : "current selection";
    const fileText = activeFile ? ` in ${activeFile}` : "";
    const prompt = `${ACTION_PROMPTS[action]}

Target: ${rangeText}${fileText}

Selected code:
\`\`\`
${selectedText}
\`\`\``;

    addMessage({
      id: `quick-${Date.now()}`,
      role: "user",
      content: prompt,
      timestamp: Date.now(),
    });

    await sendPrompt({
      prompt,
      contextFiles: activeFile ? [activeFile] : [],
      activeFile: activeFile ?? undefined,
      activeFileContent: activeFile ? fileContents[activeFile] : undefined,
      selection: selectedText,
    });
  };

  // 无选区或位置计算失败 → 不渲染
  if (!selectedText || !position) return null;

  return (
    <div
      className="absolute z-20 animate-fade-in pointer-events-auto"
      style={{
        top: position.top,
        left: position.left,
        transform: "translateX(-50%)",
      }}
    >
      <div className="flex gap-0.5 bg-surface-panel border border-surface-border rounded-lg p-1 shadow-lg">
        {actions.map((action) => (
          <button
            key={action.key}
            onClick={(e) => {
              e.stopPropagation();
              void runQuickAction(action.key);
            }}
            disabled={isAgentBusy}
            className="flex items-center gap-1 px-2 py-1 text-xs text-surface-muted hover:text-surface-text hover:bg-surface-border/50 rounded transition-colors whitespace-nowrap"
          >
            <span>{action.icon}</span>
            <span>{action.label}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
