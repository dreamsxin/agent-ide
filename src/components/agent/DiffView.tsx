import { useCallback } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import type { DiffEntry } from "../../types/agent";

function HunkBlock({ hunk }: { hunk: DiffEntry["hunks"][0] }) {
  const lines = hunk.content.split("\n");

  return (
    <div className="text-xs font-mono leading-relaxed">
      {lines.map((line, i) => {
        let bg = "";
        if (line.startsWith("+")) {
          bg = "bg-diff-add/15";
        } else if (line.startsWith("-")) {
          bg = "bg-diff-remove/15";
        }

        return (
          <div key={i} className={`px-2 ${bg}`}>
            <span className="whitespace-pre">{line}</span>
          </div>
        );
      })}
    </div>
  );
}

export default function DiffView() {
  const diffs = useAgentStore((s) => s.diffs);
  const applyAllDiffs = useAgentStore((s) => s.applyAllDiffs);
  const rejectAllDiffs = useAgentStore((s) => s.rejectAllDiffs);

  const pendingDiffs = diffs.filter((d) => d.status === "pending");
  const hasPending = pendingDiffs.length > 0;

  const handleApplyAll = useCallback(async () => {
    await applyAllDiffs();
  }, [applyAllDiffs]);

  const handleRejectAll = useCallback(async () => {
    await rejectAllDiffs();
  }, [rejectAllDiffs]);

  return (
    <div className="p-2 space-y-3 animate-fade-in h-full flex flex-col">
      {/* Bulk actions */}
      {hasPending && (
        <div className="flex gap-2 flex-shrink-0">
          <button
            onClick={handleApplyAll}
            className="flex-1 px-2 py-1 text-xs bg-diff-add/20 text-diff-add border border-diff-add/40 rounded hover:bg-diff-add/30 transition-colors"
          >
            ✓ Apply All ({pendingDiffs.length})
          </button>
          <button
            onClick={handleRejectAll}
            className="flex-1 px-2 py-1 text-xs bg-diff-remove/20 text-diff-remove border border-diff-remove/40 rounded hover:bg-diff-remove/30 transition-colors"
          >
            ✕ Reject All
          </button>
        </div>
      )}

      {/* Diff 列表 */}
      <div className="flex-1 overflow-auto space-y-2">
        {diffs.length > 0 ? (
          diffs.map((diff) => (
            <div
              key={diff.id}
              className="border border-surface-border rounded-lg overflow-hidden bg-surface-base"
            >
              {/* 文件头 */}
              <div className="flex items-center justify-between px-3 py-2 bg-surface-panel border-b border-surface-border">
                <span className="text-xs font-medium text-surface-text truncate flex-1">
                  {diff.file}
                </span>
                <span className="text-[10px] text-diff-add mr-1">
                  +{diff.hunks.reduce((sum, h) => sum + h.newLines, 0)}
                </span>
                <span className="text-[10px] text-diff-remove">
                  -{diff.hunks.reduce((sum, h) => sum + h.oldLines, 0)}
                </span>
              </div>

              {/* Diff 内容 */}
              <div className="overflow-auto max-h-60">
                {diff.hunks.map((hunk, i) => (
                  <HunkBlock key={i} hunk={hunk} />
                ))}
              </div>

              {/* 状态指示 */}
              {diff.status === "applied" && (
                <div className="px-3 py-1 bg-diff-add/10 text-diff-add text-xs text-center">
                  ✓ Applied
                </div>
              )}
              {diff.status === "rejected" && (
                <div className="px-3 py-1 bg-diff-remove/10 text-diff-remove text-xs text-center">
                  ✕ Rejected
                </div>
              )}
            </div>
          ))
        ) : (
          <div className="text-xs text-surface-muted text-center py-10">
            <div>No pending changes</div>
            <div className="text-[10px] mt-1">
              Code changes suggested by the Agent will appear here.
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
