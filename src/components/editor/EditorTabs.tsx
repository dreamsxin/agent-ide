import { useEditorStore } from "../../stores/useEditorStore";
import { useT } from "../../i18n";

export default function EditorTabs() {
  const t = useT();
  const openFiles = useEditorStore((s) => s.openFiles);
  const activeFile = useEditorStore((s) => s.activeFile);
  const setActiveFile = useEditorStore((s) => s.setActiveFile);
  const closeFile = useEditorStore((s) => s.closeFile);

  if (openFiles.length === 0) return null;

  return (
    <div
      role="tablist"
      aria-label={t("editor.tabs.openFiles")}
      className="flex items-center bg-surface-panel border-b border-surface-border overflow-x-auto no-select"
    >
      {openFiles.map((file) => (
        <div
          key={file.path}
          role="tab"
          tabIndex={0}
          aria-selected={activeFile === file.path}
          onClick={() => setActiveFile(file.path)}
          // 标签页里嵌着关闭按钮，所以不能直接用 <button>（嵌套按钮是非法 HTML）。
          // 用 role="tab" + tabIndex + 键盘处理，否则纯键盘用户切不了标签页。
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              setActiveFile(file.path);
            }
          }}
          className={`group flex items-center gap-1.5 px-3 py-1.5 text-xs border-r border-surface-border cursor-pointer transition-colors min-w-0 ${
            activeFile === file.path
              ? "bg-surface-base text-surface-text border-t-2 border-t-accent-blue"
              : "text-surface-muted hover:text-surface-text hover:bg-surface-border/30"
          }`}
        >
          <span className="truncate max-w-[120px]">{file.name}</span>
          {file.isDirty && (
            <span className="w-1.5 h-1.5 rounded-full bg-accent-blue flex-shrink-0" />
          )}
          {/* 读取失败过的标签必须看得出来：它的缓冲区不代表磁盘上的文件，也不能保存 */}
          {file.loadError && (
            <span
              title={t("editor.tab.loadFailed", { error: file.loadError })}
              aria-label={t("editor.tab.loadFailed.aria", { name: file.name })}
              className="flex-shrink-0 font-mono text-[10px] leading-none text-diff-remove"
            >
              !
            </span>
          )}
          <button
            onClick={(e) => {
              e.stopPropagation();
              closeFile(file.path);
            }}
            aria-label={t("editor.tab.close", { name: file.name })}
            title={t("editor.tab.close", { name: file.name })}
            className="opacity-0 group-hover:opacity-100 text-surface-muted hover:text-surface-text ml-0.5 flex-shrink-0"
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
