import { useEffect, useRef } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useMonacoContext } from "./MonacoContext";
import type { editor, IDisposable } from "monaco-editor";

/**
 * AI 意图提示层 —— 在特定行显示 AI 建议/警告/优化提示
 *
 * IntentHint 数据结构：
 *   line    — 目标行号
 *   message — 提示消息
 *   type    — "optimize" | "warning" | "info" | "security"
 */
export default function IntentHint() {
  const intentHints = useEditorStore((s) => s.intentHints);
  const { editor, monaco } = useMonacoContext();
  const widgetsRef = useRef<editor.IContentWidget[]>([]);
  const disposablesRef = useRef<IDisposable[]>([]);

  useEffect(() => {
    if (!editor || !monaco) return;

    const model = editor.getModel();
    if (!model) return;

    // 清除旧 widgets
    for (const widget of widgetsRef.current) {
      editor.removeContentWidget(widget);
    }
    for (const d of disposablesRef.current) {
      d.dispose();
    }
    widgetsRef.current = [];
    disposablesRef.current = [];

    // 为每个 hint 创建 content widget
    for (const hint of intentHints) {
      const domNode = document.createElement("div");
      domNode.className = `intent-hint-widget intent-hint--${hint.type}`;
      domNode.innerHTML = `
        <span class="intent-hint-icon">${getHintIcon(hint.type)}</span>
        <span class="intent-hint-text">${escapeHtml(hint.message)}</span>
      `;

      const widget: editor.IContentWidget = {
        getId: () => `intent-hint-${hint.line}`,
        getDomNode: () => domNode,
        getPosition: () => ({
          position: { lineNumber: hint.line, column: 1 },
          preference: [monaco.editor.ContentWidgetPositionPreference.BELOW],
        }),
      };

      editor.addContentWidget(widget);
      widgetsRef.current.push(widget);
    }
  }, [intentHints, editor, monaco]);

  return null;
}

function getHintIcon(type: string): string {
  switch (type) {
    case "optimize":
      return "⚡";
    case "warning":
      return "⚠️";
    case "security":
      return "🔒";
    case "info":
    default:
      return "💡";
  }
}

function escapeHtml(text: string): string {
  const map: Record<string, string> = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#039;",
  };
  return text.replace(/[&<>"']/g, (c) => map[c] || c);
}
