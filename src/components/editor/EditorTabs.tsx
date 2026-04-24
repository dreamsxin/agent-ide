import { useEditorStore } from "../../stores/useEditorStore";

export default function EditorTabs() {
  const openFiles = useEditorStore((s) => s.openFiles);
  const activeFile = useEditorStore((s) => s.activeFile);
  const setActiveFile = useEditorStore((s) => s.setActiveFile);
  const closeFile = useEditorStore((s) => s.closeFile);

  if (openFiles.length === 0) return null;

  return (
    <div className="flex items-center bg-surface-panel border-b border-surface-border overflow-x-auto no-select">
      {openFiles.map((file) => (
        <div
          key={file.path}
          onClick={() => setActiveFile(file.path)}
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
          <button
            onClick={(e) => {
              e.stopPropagation();
              closeFile(file.path);
            }}
            className="opacity-0 group-hover:opacity-100 text-surface-muted hover:text-surface-text ml-0.5 flex-shrink-0"
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
