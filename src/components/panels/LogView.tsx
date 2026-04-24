import { useEffect, useRef } from "react";
import { useLogStore } from "../../stores/useLogStore";

const LEVEL_COLORS: Record<string, string> = {
  info: "text-accent-blue",
  warn: "text-diff-modify",
  error: "text-diff-remove",
  success: "text-accent-green",
};

const SOURCE_ICONS: Record<string, string> = {
  agent: "🤖",
  git: "⬢",
  fs: "📁",
  system: "⚙",
};

export default function LogView() {
  const logs = useLogStore((s) => s.logs);
  const clearLogs = useLogStore((s) => s.clearLogs);
  const containerRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom
  useEffect(() => {
    if (containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [logs]);

  if (logs.length === 0) {
    return (
      <div className="h-full flex items-center justify-center bg-black text-surface-muted font-mono text-xs">
        <div className="text-center">
          <div className="text-2xl mb-2">📋</div>
          <div>No logs yet</div>
          <div className="text-[10px] mt-1">Agent activity will appear here</div>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-black">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-2 py-1 border-b border-surface-border/20">
        <span className="text-surface-muted text-[10px] font-mono">
          {logs.length} entries
        </span>
        <button
          onClick={clearLogs}
          className="text-surface-muted hover:text-surface-text text-[10px] px-1"
        >
          Clear
        </button>
      </div>

      {/* Log entries */}
      <div ref={containerRef} className="flex-1 overflow-auto p-2 font-mono text-xs">
        {logs.map((log) => (
          <div
            key={log.id}
            className="flex gap-2 py-0.5 hover:bg-surface-border/10 transition-colors group"
          >
            {/* Source icon */}
            <span className="flex-shrink-0 text-[10px] w-4 text-center" title={log.source}>
              {SOURCE_ICONS[log.source] ?? "•"}
            </span>

            {/* Timestamp */}
            <span className="text-surface-muted flex-shrink-0 w-16 text-[10px]">
              {log.time}
            </span>

            {/* Level badge */}
            <span
              className={`flex-shrink-0 w-10 text-[10px] font-semibold ${
                LEVEL_COLORS[log.level] ?? "text-surface-muted"
              }`}
            >
              {log.level.toUpperCase()}
            </span>

            {/* Message */}
            <span className="text-surface-text min-w-0 truncate">{log.message}</span>

            {/* Source label */}
            <span className="ml-auto text-surface-muted text-[9px] opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0">
              {log.source}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
