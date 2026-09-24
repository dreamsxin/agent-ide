import { useEffect, useRef, useState } from "react";
import { useLogStore } from "../../stores/useLogStore";
import { useT } from "../../i18n";
import type { MessageKey } from "../../i18n/messages";

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
  const t = useT();
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
          <div>{t("logs.empty")}</div>
          <div className="text-[10px] mt-1">{t("logs.empty.hint")}</div>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-black">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-2 py-1 border-b border-surface-border/20">
        <span className="text-surface-muted text-[10px] font-mono">
          {t("logs.entries", { count: logs.length })}
        </span>
        <button
          onClick={clearLogs}
          className="text-surface-muted hover:text-surface-text text-[10px] px-1"
        >
          {t("logs.clear")}
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
                  {hasDetails ? (isExpanded ? t("logs.hide") : t("logs.details")) : log.source}
                </span>
              </button>

              {isExpanded && (
                <div className="ml-[5.75rem] mr-2 mb-1 rounded border border-surface-border/30 bg-surface-base/80 p-2 text-[10px] text-surface-muted whitespace-pre-wrap break-words">
                  <MetaLine labelKey="logs.meta.phase" value={log.phase} />
                  <MetaLine labelKey="logs.meta.role" value={log.role ?? undefined} />
                  <MetaLine labelKey="logs.meta.stage" value={log.stage ?? undefined} />
                  {log.details && (
                    <Section titleKey="logs.section.details" content={log.details} />
                  )}
                  {log.contextSummary && (
                    <Section titleKey="logs.section.context" content={log.contextSummary} />
                  )}
                  {log.diffSummary && (
                    <Section titleKey="logs.section.diffs" content={log.diffSummary} />
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

function MetaLine({ labelKey, value }: { labelKey: MessageKey; value?: string | null }) {
  const t = useT();
  if (!value) return null;
  return (
    <div className="mb-1">
      <span className="text-surface-text">{t(labelKey)}:</span> {value}
    </div>
  );
}

function Section({ titleKey, content }: { titleKey: MessageKey; content: string }) {
  const t = useT();
  return (
    <div className="mt-2">
      <div className="text-surface-text mb-1">{t(titleKey)}</div>
      <div>{content}</div>
    </div>
  );
}
