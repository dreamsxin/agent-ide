import { useEffect, useRef } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useMonacoContext } from "./MonacoContext";

/**
 * Ghost Text 装饰层 —— 在光标后在编辑器内显示内联 AI 建议
 * 监听 useEditorStore.inlineSuggestion, 用 Monaco decorations API 渲染
 */
export default function InlineSuggestion() {
  const inlineSuggestion = useEditorStore((s) => s.inlineSuggestion);
  const { editor, monaco } = useMonacoContext();
  const decorationsRef = useRef<string[]>([]);

  useEffect(() => {
    if (!editor || !monaco) return;

    const model = editor.getModel();
    if (!model) return;

    decorationsRef.current = editor.deltaDecorations(decorationsRef.current, []);

    if (!inlineSuggestion) return;

    const { line, column, text } = inlineSuggestion;

    decorationsRef.current = editor.deltaDecorations(decorationsRef.current, [
      {
        range: new monaco.Range(line, column, line, column),
        options: {
          after: {
            content: text,
            inlineClassName: "ghost-text-decoration",
          },
        },
      },
    ]);
  }, [inlineSuggestion, editor, monaco]);

  // 无视觉渲染 — 仅通过 decorations API 工作
  return null;
}
