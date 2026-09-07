import { useCallback } from "react";
import { isReviewableDiff, useAgentStore } from "../../stores/useAgentStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import type { DiffEntry } from "../../types/agent";

/**
 * 对话流里的「有改动待审查」卡片。
 *
 * 分层是刻意的：这里只负责**通知 + 快速决策**，逐 hunk、provenance、baseHash
 * 那些深度信息留在 Changes 面板。
 *
 * 之前对话流里没有任何这一层：Suggest / Ask 模式下 Agent 只提交待审查的 diff、
 * 不落盘，而唯一的提示是 Changes 标签上一个 11px 角标。用户看完 Agent 的回复
 * 会以为事情做完了，或者以为什么都没做 —— 两种误解都出现过。
 */
export default function PendingChangesCard() {
  const diffs = useAgentStore((s) => s.diffs);
  const applyDiff = useAgentStore((s) => s.applyDiff);
  const rejectDiff = useAgentStore((s) => s.rejectDiff);
  const applyAllDiffs = useAgentStore((s) => s.applyAllDiffs);
  const setAgentView = useLayoutStore((s) => s.setAgentView);

  const pending = diffs.filter(isReviewableDiff);

  const openReview = useCallback(() => setAgentView("changes"), [setAgentView]);

  if (pending.length === 0) return null;

  return (
    <div
      data-testid="chat-pending-changes"
      className="rounded border border-diff-modify/40 bg-diff-modify/5 p-2 text-xs"
    >
      <div className="mb-1.5 flex items-center gap-2">
        <span className="font-medium text-surface-text">
          {pending.length} file{pending.length === 1 ? "" : "s"} waiting for review
        </span>
        <span className="text-[11px] text-surface-muted">nothing written to disk yet</span>
      </div>

      <div className="space-y-1">
        {pending.map((diff) => (
          <div key={diff.id} className="flex items-center gap-2">
            <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-surface-text">
              {diff.file}
            </span>
            <LineDelta diff={diff} />
            <button
              type="button"
              data-testid="chat-apply-diff"
              onClick={() => void applyDiff(diff.id)}
              className="rounded px-1.5 py-0.5 text-[11px] text-diff-add hover:bg-diff-add/10"
            >
              Apply
            </button>
            <button
              type="button"
              onClick={() => void rejectDiff(diff.id)}
              className="rounded px-1.5 py-0.5 text-[11px] text-diff-remove hover:bg-diff-remove/10"
            >
              Reject
            </button>
          </div>
        ))}
      </div>

      <div className="mt-2 flex items-center gap-2">
        <button
          type="button"
          data-testid="chat-apply-all"
          onClick={() => void applyAllDiffs()}
          className="rounded bg-accent-blue px-2 py-0.5 text-[11px] text-white hover:bg-accent-blue/80"
        >
          Apply all ({pending.length})
        </button>
        {/* 深度审查（逐 hunk、provenance、baseHash）在 Changes 面板，这里只给入口 */}
        <button
          type="button"
          onClick={openReview}
          className="rounded border border-surface-border px-2 py-0.5 text-[11px] text-surface-muted hover:text-surface-text"
        >
          Review hunk by hunk
        </button>
      </div>
    </div>
  );
}

function LineDelta({ diff }: { diff: DiffEntry }) {
  let added = 0;
  let removed = 0;
  for (const hunk of diff.hunks) {
    if (hunk.updated) added += hunk.updated.split("\n").length;
    if (hunk.original) removed += hunk.original.split("\n").length;
  }
  return (
    <span className="flex-shrink-0 font-mono text-[10px]">
      <span className="text-diff-add">+{added}</span>{" "}
      <span className="text-diff-remove">-{removed}</span>
    </span>
  );
}
