import { useMemo } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useMonacoContext } from "./MonacoContext";

const actions = [
  { key: "explain", label: "Explain", icon: "💡" },
  { key: "fix", label: "Fix", icon: "🔧" },
  { key: "refactor", label: "Refactor", icon: "♻️" },
  { key: "optimize", label: "Optimize", icon: "⚡" },
] as const;

/**
 * 选区浮动工具栏 —— 选中文本时在选区上方弹出 [Explain | Fix | Refactor | Optimize]
 */
export default function QuickActions() {
  const selectedText = useEditorStore((s) => s.selectedText);
  const selectedRange = useEditorStore((s) => s.selectedRange);
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
              // TODO: 触发 Agent 对应操作
              console.log(`${action.key}:`, selectedText);
            }}
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
