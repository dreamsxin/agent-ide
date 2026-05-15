import { useCallback } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import type { DiffEntry } from "../../types/agent";

function HunkBlock({ hunk }: { hunk: DiffEntry["hunks"][0] }) {
  const hasOriginal = hunk.original && hunk.original.trim().length > 0;
  const hasUpdated = hunk.updated && hunk.updated.trim().length > 0;

  if (!hasOriginal && hasUpdated) {
    const lines = hunk.updated.split("\n");
    return (
      <div className="text-xs font-mono leading-relaxed">
        <div className="border-b border-diff-add/20 bg-diff-add/10 px-2 py-0.5 text-[10px] font-semibold text-diff-add">
          + New file
        </div>
        {lines.map((line, i) => (
          <div key={i} className="bg-diff-add/5 px-2">
            <span className="whitespace-pre text-diff-add">+ {line}</span>
          </div>
        ))}
      </div>
    );
  }

  if (hasOriginal && hasUpdated) {
    const origLines = hunk.original.split("\n");
    const updLines = hunk.updated.split("\n");

    return (
      <div className="text-xs font-mono leading-relaxed">
        <div className="grid grid-cols-2 border-b border-surface-border">
          <div className="bg-diff-remove/10 px-2 py-0.5 text-[10px] font-semibold text-diff-remove">
            - Original
          </div>
          <div className="border-l border-surface-border bg-diff-add/10 px-2 py-0.5 text-[10px] font-semibold text-diff-add">
            + Updated
          </div>
        </div>
        <div className="grid grid-cols-2">
          <div className="bg-diff-remove/5">
            {origLines.map((line, i) => (
              <div key={i} className="border-r border-surface-border/50 px-2">
                <span className="whitespace-pre text-diff-remove">{line || " "}</span>
              </div>
            ))}
          </div>
          <div className="bg-diff-add/5">
            {updLines.map((line, i) => (
              <div key={i} className="px-2">
                <span className="whitespace-pre text-diff-add">{line || " "}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    );
  }

  const lines = hunk.content.split("\n");
  return (
    <div className="text-xs font-mono leading-relaxed">
      {lines.map((line, i) => {
        let bg = "";
        if (line.startsWith("+")) bg = "bg-diff-add/15";
        else if (line.startsWith("-")) bg = "bg-diff-remove/15";

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
  const lastApplyResult = useAgentStore((s) => s.lastApplyResult);
  const clearApplyResult = useAgentStore((s) => s.clearApplyResult);
  const applyAllDiffs = useAgentStore((s) => s.applyAllDiffs);
  const applyDiff = useAgentStore((s) => s.applyDiff);
  const rejectAllDiffs = useAgentStore((s) => s.rejectAllDiffs);
  const rejectDiff = useAgentStore((s) => s.rejectDiff);

  const pendingDiffs = diffs.filter((d) => d.status === "pending");
  const hasPending = pendingDiffs.length > 0;
  const failedMessages = new Map(
    (lastApplyResult?.failed ?? []).map((item) => [item.diffId, item.message])
  );

  const handleApplyAll = useCallback(async () => {
    await applyAllDiffs();
  }, [applyAllDiffs]);

  const handleRejectAll = useCallback(async () => {
    await rejectAllDiffs();
  }, [rejectAllDiffs]);

  const handleApplyDiff = useCallback(
    async (diffId: string) => {
      await applyDiff(diffId);
    },
    [applyDiff]
  );

  const handleRejectDiff = useCallback(
    async (diffId: string) => {
      await rejectDiff(diffId);
    },
    [rejectDiff]
  );

  return (
    <div className="flex h-full flex-col space-y-3 p-2 animate-fade-in">
      {lastApplyResult && lastApplyResult.failed.length > 0 && (
        <div className="flex-shrink-0 rounded border border-diff-remove/40 bg-diff-remove/10 p-2 text-xs text-diff-remove">
          <div className="mb-1 flex items-center justify-between gap-2">
            <span>Some diffs could not be applied.</span>
            <button
              onClick={clearApplyResult}
              className="rounded border border-diff-remove/30 px-1.5 py-0.5 text-[10px] hover:bg-diff-remove/10"
            >
              Dismiss
            </button>
          </div>
          <div className="space-y-1">
            {lastApplyResult.failed.map((item) => (
              <div key={item.diffId} className="rounded bg-surface-base/60 px-2 py-1">
                <div className="font-medium text-surface-text">{item.file}</div>
                <div className="mt-0.5 break-words text-[11px] text-diff-remove">
                  {item.message}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {hasPending && (
        <div className="flex flex-shrink-0 gap-2">
          <button
            onClick={handleApplyAll}
            className="flex-1 rounded border border-diff-add/40 bg-diff-add/20 px-2 py-1 text-xs text-diff-add transition-colors hover:bg-diff-add/30"
          >
            Apply All ({pendingDiffs.length})
          </button>
          <button
            onClick={handleRejectAll}
            className="flex-1 rounded border border-diff-remove/40 bg-diff-remove/20 px-2 py-1 text-xs text-diff-remove transition-colors hover:bg-diff-remove/30"
          >
            Reject All
          </button>
        </div>
      )}

      <div className="flex-1 space-y-2 overflow-auto">
        {diffs.length > 0 ? (
          diffs.map((diff) => (
            <div
              key={diff.id}
              className="overflow-hidden rounded-lg border border-surface-border bg-surface-base"
            >
              <div className="border-b border-surface-border bg-surface-panel px-3 py-2">
                <div className="flex items-center justify-between gap-2">
                  <span className="flex-1 truncate text-xs font-medium text-surface-text">
                    {diff.file}
                  </span>
                  <span className="mr-1 text-[10px] text-diff-add">
                    +{diff.hunks.reduce((sum, h) => sum + h.newLines, 0)}
                  </span>
                  <span className="text-[10px] text-diff-remove">
                    -{diff.hunks.reduce((sum, h) => sum + h.oldLines, 0)}
                  </span>
                </div>
                {diff.baseHash && (
                  <div className="mt-1 flex items-center gap-1 text-[10px] text-surface-muted">
                    <span className="rounded border border-surface-border px-1 py-0.5">
                      baseHash
                    </span>
                    <span className="truncate font-mono">{diff.baseHash}</span>
                  </div>
                )}
                {diff.status === "pending" && (
                  <div className="mt-2 flex gap-2">
                    <button
                      onClick={() => handleApplyDiff(diff.id)}
                      className="rounded border border-diff-add/40 bg-diff-add/15 px-2 py-1 text-[11px] text-diff-add transition-colors hover:bg-diff-add/25"
                    >
                      Apply
                    </button>
                    <button
                      onClick={() => handleRejectDiff(diff.id)}
                      className="rounded border border-diff-remove/40 bg-diff-remove/15 px-2 py-1 text-[11px] text-diff-remove transition-colors hover:bg-diff-remove/25"
                    >
                      Reject
                    </button>
                  </div>
                )}
              </div>

              <div className="max-h-60 overflow-auto">
                {diff.hunks.map((hunk, i) => (
                  <HunkBlock key={i} hunk={hunk} />
                ))}
              </div>

              {diff.status === "applied" && (
                <div className="bg-diff-add/10 px-3 py-1 text-center text-xs text-diff-add">
                  Applied
                </div>
              )}
              {diff.status === "rejected" && (
                <div className="bg-diff-remove/10 px-3 py-1 text-center text-xs text-diff-remove">
                  Rejected
                </div>
              )}
              {diff.status === "failed" && (
                <div className="bg-diff-remove/10 px-3 py-1 text-xs text-diff-remove">
                  <div className="font-medium">Apply failed</div>
                  {(diff.applyError || failedMessages.get(diff.id)) && (
                    <div className="mt-0.5 break-words text-[11px]">
                      {diff.applyError || failedMessages.get(diff.id)}
                    </div>
                  )}
                  {isHashMismatch(diff.applyError || failedMessages.get(diff.id)) && (
                    <div className="mt-1 rounded border border-diff-remove/30 bg-surface-base/70 px-2 py-1 text-[10px] text-surface-muted">
                      The file changed after the Agent generated this diff. Ask the Agent to regenerate the change against the current file before applying.
                    </div>
                  )}
                </div>
              )}
            </div>
          ))
        ) : (
          <div className="py-10 text-center text-xs text-surface-muted">
            <div>No pending changes</div>
            <div className="mt-1 text-[10px]">
              Code changes suggested by the Agent will appear here.
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function isHashMismatch(message?: string) {
  if (!message) return false;
  return message.includes("baseHash") || message.includes("File changed since diff was generated");
}
