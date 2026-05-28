import { useCallback } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useProblemStore, type ProblemEntry } from "../../stores/useProblemStore";
import type { DiffEntry, DiffHunk } from "../../types/agent";

function HunkBlock({
  hunk,
  index,
  diffStatus,
  onApply,
  onReject,
  onRegenerate,
  findings,
}: {
  hunk: DiffHunk;
  index: number;
  diffStatus: DiffEntry["status"];
  onApply: () => void;
  onReject: () => void;
  onRegenerate: () => void;
  findings: ProblemEntry[];
}) {
  const hasOriginal = hunk.original && hunk.original.trim().length > 0;
  const hasUpdated = hunk.updated && hunk.updated.trim().length > 0;
  const hunkStatus = hunk.status ?? "pending";
  const canAct = isReviewableDiffStatus(diffStatus) && hunkStatus !== "applied" && hunkStatus !== "rejected";
  const provenanceLabel = [hunk.provenance?.sourceRole, hunk.provenance?.sourceStage]
    .filter(Boolean)
    .join(" / ");

  const header = (
    <div className="flex items-center justify-between gap-2 border-b border-surface-border bg-surface-panel/70 px-2 py-1">
      <span className="text-[10px] font-semibold uppercase text-surface-muted">
        Hunk {index + 1} · {hunkStatus}
      </span>
      {canAct && (
        <span className="flex items-center gap-1">
          <button
            onClick={onApply}
            data-testid="apply-hunk"
            className="rounded border border-diff-add/40 px-1.5 py-0.5 text-[10px] text-diff-add hover:bg-diff-add/10"
          >
            Apply hunk
          </button>
          <button
            onClick={onReject}
            data-testid="reject-hunk"
            className="rounded border border-diff-remove/40 px-1.5 py-0.5 text-[10px] text-diff-remove hover:bg-diff-remove/10"
          >
            Reject hunk
          </button>
          {diffStatus === "failed" && (
            <button
              onClick={onRegenerate}
              className="rounded border border-accent-blue/40 px-1.5 py-0.5 text-[10px] text-accent-blue hover:bg-accent-blue/10"
            >
              Regenerate
            </button>
          )}
        </span>
      )}
      {findings.length > 0 && (
        <span className="rounded border border-accent-blue/30 bg-accent-blue/10 px-1.5 py-0.5 text-[10px] text-accent-blue">
          {findings.length} finding{findings.length === 1 ? "" : "s"}
        </span>
      )}
    </div>
  );

  const findingPanel = findings.length > 0 && (
    <div className="border-b border-accent-blue/20 bg-accent-blue/5 px-2 py-1 font-sans text-[11px] leading-snug text-surface-text">
      {findings.slice(0, 3).map((finding) => (
        <div key={finding.id} className="flex gap-1">
          <span className={problemSeverityClass(finding.severity)}>
            {finding.severity}
          </span>
          <span className="min-w-0 flex-1 truncate">
            {finding.source}: {finding.message}
          </span>
        </div>
      ))}
      {findings.length > 3 && (
        <div className="text-surface-muted">+{findings.length - 3} more</div>
      )}
    </div>
  );

  const provenancePanel = hunk.provenance && (
    <div className="border-b border-surface-border/60 bg-surface-base/60 px-2 py-1 font-sans text-[10px] leading-snug text-surface-muted">
      {provenanceLabel && <span>{provenanceLabel}</span>}
      {hunk.provenance.changeIndex != null && (
        <span className="ml-2">change {hunk.provenance.changeIndex}</span>
      )}
      {hunk.provenance.hunkIndex != null && (
        <span className="ml-2">hunk {hunk.provenance.hunkIndex}</span>
      )}
      {hunk.provenance.promptContext && (
        <div className="mt-0.5 truncate">{hunk.provenance.promptContext}</div>
      )}
    </div>
  );

  if (!hasOriginal && hasUpdated) {
    const lines = hunk.updated.split("\n");
    return (
      <div data-testid={`diff-hunk-${index}`} className="border-b border-surface-border text-xs font-mono leading-relaxed">
        {header}
        {provenancePanel}
        {findingPanel}
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
      <div data-testid={`diff-hunk-${index}`} className="border-b border-surface-border text-xs font-mono leading-relaxed">
        {header}
        {provenancePanel}
        {findingPanel}
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
    <div data-testid={`diff-hunk-${index}`} className="border-b border-surface-border text-xs font-mono leading-relaxed">
      {header}
      {provenancePanel}
      {findingPanel}
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
  const applyDiffHunk = useAgentStore((s) => s.applyDiffHunk);
  const rejectAllDiffs = useAgentStore((s) => s.rejectAllDiffs);
  const rejectDiff = useAgentStore((s) => s.rejectDiff);
  const rejectDiffHunk = useAgentStore((s) => s.rejectDiffHunk);
  const regenerateDiff = useAgentStore((s) => s.regenerateDiff);
  const problems = useProblemStore((s) => s.problems);
  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const fileContents = useEditorStore((s) => s.fileContents);

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

  const handleRegenerateDiff = useCallback(
    async (diff: DiffEntry, hunkIndex?: number) => {
      const currentContent = fileContents[diff.file] ?? fileContents[activeFile ?? ""];
      await regenerateDiff({
        diff,
        hunkIndex,
        activeFile: diff.file,
        activeFileContent: currentContent,
        currentFileContent: currentContent,
        contextFiles: openFiles.map((file) => file.path),
        contextSources: {
          includeGitDiff: true,
          includeProjectTree: true,
        },
      });
    },
    [activeFile, fileContents, openFiles, regenerateDiff]
  );

  return (
    <div data-testid="diff-view" className="flex h-full flex-col space-y-3 p-2 animate-fade-in">
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
          diffs.map((diff) => {
            const counts = getHunkStatusCounts(diff.hunks);
            const fileFindings = problems.filter((problem) => problemMatchesFile(problem, diff.file));
            return (
              <div
                key={diff.id}
                data-testid="diff-card"
                className="overflow-hidden rounded-lg border border-surface-border bg-surface-base"
              >
              <div className="border-b border-surface-border bg-surface-panel px-3 py-2">
                <div className="flex items-center justify-between gap-2">
                  <span className="flex-1 truncate text-xs font-medium text-surface-text">
                    {diff.file}
                  </span>
                  <span className={diffStatusClass(diff.status)}>
                    {diffStatusLabel(diff.status)}
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
                {diff.provenance && (
                  <div className="mt-1 flex flex-wrap items-center gap-1 text-[10px] text-surface-muted">
                    <span className="rounded border border-surface-border px-1 py-0.5">
                      {diff.provenance.protocol}
                    </span>
                    <span className="rounded border border-surface-border px-1 py-0.5">
                      {diff.provenance.operation}
                    </span>
                    {diff.provenance.schemaVersion != null && (
                      <span className="rounded border border-surface-border px-1 py-0.5">
                        v{diff.provenance.schemaVersion}
                      </span>
                    )}
                    {(diff.provenance.sourceRole || diff.provenance.sourceStage) && (
                      <span className="rounded border border-surface-border px-1 py-0.5">
                        {[diff.provenance.sourceRole, diff.provenance.sourceStage]
                          .filter(Boolean)
                          .join(" / ")}
                      </span>
                    )}
                  {diff.provenance.rationale && (
                    <span className="min-w-0 flex-1 truncate">
                      {diff.provenance.rationale}
                    </span>
                  )}
                    {diff.provenance.regeneratedFromDiffId && (
                      <span
                        className="rounded border border-diff-modify/40 bg-diff-modify/10 px-1 py-0.5 text-diff-modify"
                        title={
                          diff.provenance.regeneratedFromHunkIndex != null
                            ? `Regenerated from ${diff.provenance.regeneratedFromDiffId}, hunk ${diff.provenance.regeneratedFromHunkIndex}`
                            : `Regenerated from ${diff.provenance.regeneratedFromDiffId}`
                        }
                      >
                        regenerated
                      </span>
                    )}
                  </div>
                )}
                {(diff.status === "partial" || hasReviewedHunks(counts)) && (
                  <div className="mt-1 flex flex-wrap gap-1 text-[10px]">
                    <span className="text-surface-muted">Hunks:</span>
                    <HunkCount label="pending" count={counts.pending} className="border-surface-border text-surface-muted" />
                    <HunkCount label="applied" count={counts.applied} className="border-diff-add/40 text-diff-add" />
                    <HunkCount label="rejected" count={counts.rejected} className="border-diff-remove/40 text-diff-remove" />
                    <HunkCount label="failed" count={counts.failed} className="border-diff-remove/40 bg-diff-remove/10 text-diff-remove" />
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
                {diff.status === "failed" && (
                  <div className="mt-2">
                    <button
                      onClick={() => void handleRegenerateDiff(diff)}
                      className="rounded border border-accent-blue/40 bg-accent-blue/10 px-2 py-1 text-[11px] text-accent-blue transition-colors hover:bg-accent-blue/20"
                    >
                      Regenerate against current file
                    </button>
                  </div>
                )}
              </div>

              <div className="max-h-60 overflow-auto">
                {diff.hunks.map((hunk, i) => (
                  <HunkBlock
                    key={i}
                    hunk={hunk}
                    index={i}
                    diffStatus={diff.status}
                    findings={fileFindings.filter((problem) => problemMatchesHunk(problem, hunk))}
                    onApply={() => void applyDiffHunk(diff.id, i)}
                    onReject={() => void rejectDiffHunk(diff.id, i)}
                    onRegenerate={() => void handleRegenerateDiff(diff, i)}
                  />
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
            );
          })
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

function HunkCount({ label, count, className }: { label: string; count: number; className: string }) {
  return (
    <span className={`rounded border px-1 py-0.5 ${className}`}>
      {count} {label}
    </span>
  );
}

function isReviewableDiffStatus(status: DiffEntry["status"]) {
  return status === "pending" || status === "partial" || status === "failed";
}

function getHunkStatusCounts(hunks: DiffHunk[]) {
  return hunks.reduce(
    (counts, hunk) => {
      const status = hunk.status ?? "pending";
      counts[status] += 1;
      return counts;
    },
    { pending: 0, applied: 0, rejected: 0, failed: 0 } as Record<NonNullable<DiffHunk["status"]>, number>
  );
}

function hasReviewedHunks(counts: Record<NonNullable<DiffHunk["status"]>, number>) {
  return counts.applied > 0 || counts.rejected > 0 || counts.failed > 0;
}

function diffStatusLabel(status: DiffEntry["status"]) {
  if (status === "partial") return "Partial";
  return status.charAt(0).toUpperCase() + status.slice(1);
}

function diffStatusClass(status: DiffEntry["status"]) {
  const base = "rounded border px-1.5 py-0.5 text-[10px]";
  if (status === "applied") return `${base} border-diff-add/40 bg-diff-add/10 text-diff-add`;
  if (status === "rejected" || status === "failed") {
    return `${base} border-diff-remove/40 bg-diff-remove/10 text-diff-remove`;
  }
  if (status === "partial") return `${base} border-accent-blue/40 bg-accent-blue/10 text-accent-blue`;
  return `${base} border-surface-border bg-surface-base text-surface-muted`;
}

function problemSeverityClass(severity: ProblemEntry["severity"]) {
  if (severity === "error") return "shrink-0 font-semibold text-diff-remove";
  if (severity === "warning") return "shrink-0 font-semibold text-yellow-400";
  return "shrink-0 font-semibold text-accent-blue";
}

function problemMatchesFile(problem: ProblemEntry, file: string) {
  return normalizeProblemPath(problem.file) === normalizeProblemPath(file);
}

function problemMatchesHunk(problem: ProblemEntry, hunk: DiffHunk) {
  if (!problem.line || problem.line < 1) return true;
  const start = hunk.newStart || hunk.oldStart || 1;
  const lineCount = Math.max(hunk.newLines, hunk.oldLines, 1);
  return problem.line >= start && problem.line <= start + lineCount - 1;
}

function normalizeProblemPath(path: string) {
  return decodeURIComponent(path)
    .replace(/^file:\/+/i, "")
    .replace(/\\/g, "/")
    .replace(/^\/([a-zA-Z]:\/)/, "$1")
    .toLowerCase();
}
