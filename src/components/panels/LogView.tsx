import { useEffect, useRef, useState } from "react";
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
  const [expanded, setExpanded] = useState<string | null>(null);

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
        {logs.map((log) => {
          const hasDetails = Boolean(
            log.details || log.contextSummary || log.diffSummary || log.phase || log.role || log.stage
          );
          const isExpanded = expanded === log.id;

          return (
            <div key={log.id} className="group">
              <button
                type="button"
                onClick={() => hasDetails && setExpanded(isExpanded ? null : log.id)}
                className={`w-full flex gap-2 py-0.5 text-left hover:bg-surface-border/10 transition-colors ${
                  hasDetails ? "cursor-pointer" : "cursor-default"
                }`}
              >
                <span className="flex-shrink-0 text-[10px] w-4 text-center" title={log.source}>
                  {SOURCE_ICONS[log.source] ?? "•"}
                </span>

                <span className="text-surface-muted flex-shrink-0 w-16 text-[10px]">
                  {log.time}
                </span>

                <span
                  className={`flex-shrink-0 w-10 text-[10px] font-semibold ${
                    LEVEL_COLORS[log.level] ?? "text-surface-muted"
                  }`}
                >
                  {log.level.toUpperCase()}
                </span>

                <span className="text-surface-text min-w-0 truncate">{log.message}</span>

                {log.stage && (
                  <span className="text-surface-muted text-[9px] flex-shrink-0">
                    {log.stage}
                  </span>
                )}

                <span className="ml-auto text-surface-muted text-[9px] opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0">
                  {hasDetails ? (isExpanded ? "hide" : "details") : log.source}
                </span>
              </button>

              {isExpanded && (
                <div className="ml-[5.75rem] mr-2 mb-1 rounded border border-surface-border/30 bg-surface-base/80 p-2 text-[10px] text-surface-muted whitespace-pre-wrap break-words">
                  <MetaLine label="phase" value={log.phase} />
                  <MetaLine label="role" value={log.role ?? undefined} />
                  <MetaLine label="stage" value={log.stage ?? undefined} />
                  {log.details && (
                    <Section title="details" content={log.details} />
                  )}
                  {log.contextSummary && (
                    <Section title="context" content={log.contextSummary} />
                  )}
                  {log.diffSummary && (
                    <Section title="diffs" content={log.diffSummary} />
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function MetaLine({ label, value }: { label: string; value?: string | null }) {
  if (!value) return null;
  return (
    <div className="mb-1">
      <span className="text-surface-text">{label}:</span> {value}
    </div>
  );
}

function Section({ title, content }: { title: string; content: string }) {
  return (
    <div className="mt-2">
      <div className="text-surface-text mb-1">{title}</div>
      <div>{content}</div>
    </div>
  );
}
