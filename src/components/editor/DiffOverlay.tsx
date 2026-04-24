import { useEffect, useRef } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useMonacoContext } from "./MonacoContext";

/**
 * Diff 高亮装饰层 —— 用绿/红背景标注 AI 建议的代码差异
 *
 * DiffOverlay 数据结构：
 *   file     — 目标文件路径
 *   oldText  — 将被删除的原始文本（红色）
 *   newText  — 将被添加的新文本（绿色）
 *   startLine — 起始行号
 */
export default function DiffOverlay() {
  const diffOverlays = useEditorStore((s) => s.diffOverlays);
  const { editor, monaco } = useMonacoContext();
  const decorationsRef = useRef<string[]>([]);

  useEffect(() => {
    if (!editor || !monaco) return;

    const model = editor.getModel();
    if (!model) return;

    // 先清除旧 decorations
    decorationsRef.current = editor.deltaDecorations(decorationsRef.current, []);

    const newDecorations: monaco.editor.IModelDeltaDecoration[] = [];

    for (const overlay of diffOverlays) {
      const oldLines = overlay.oldText.split("\n").length;
      const newLines = overlay.newText.split("\n").length;

      // 红色背景 — 将被删除的行
      if (overlay.oldText) {
        newDecorations.push({
          range: new monaco.Range(
            overlay.startLine,
            1,
            overlay.startLine + oldLines - 1,
            1
          ),
          options: {
            isWholeLine: true,
            className: "diff-removed-line",
            glyphMarginClassName: "diff-removed-glyph",
          },
        });
      }

      // 绿色背景 — 将被添加的行
      if (overlay.newText) {
        newDecorations.push({
          range: new monaco.Range(
            overlay.startLine,
            1,
            overlay.startLine + newLines - 1,
            1
          ),
          options: {
            isWholeLine: true,
            className: "diff-added-line",
            glyphMarginClassName: "diff-added-glyph",
          },
        });
      }
    }

    decorationsRef.current = editor.deltaDecorations(
      decorationsRef.current,
      newDecorations
    );
  }, [diffOverlays, editor, monaco]);

  return null;
}
