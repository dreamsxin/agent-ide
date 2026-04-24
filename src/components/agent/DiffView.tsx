import type { DiffEntry } from "../../types/agent";

/** mock diff 数据 */
const mockDiff: DiffEntry = {
  id: "diff-1",
  file: "src/auth/jwt.ts",
  hunks: [
    {
      oldStart: 12,
      oldLines: 3,
      newStart: 12,
      newLines: 6,
      content: `-  const secret = process.env.JWT_SECRET;
-  return jwt.sign(payload, secret);
+  const secret = process.env.JWT_SECRET || 'fallback-secret';
+  if (!secret) {
+    throw new Error('JWT_SECRET not configured');
+  }
+  return jwt.sign(payload, secret, { expiresIn: '24h' });`,
    },
  ],
  status: "pending",
};

function HunkBlock({ hunk }: { hunk: DiffEntry["hunks"][0] }) {
  const lines = hunk.content.split("\n");

  return (
    <div className="text-xs font-mono leading-relaxed">
      {lines.map((line, i) => {
        let bg = "";
        let prefix = "";
        if (line.startsWith("+")) {
          bg = "bg-diff-add/15";
          prefix = "text-diff-add";
        } else if (line.startsWith("-")) {
          bg = "bg-diff-remove/15";
          prefix = "text-diff-remove";
        }

        return (
          <div key={i} className={`px-2 ${bg}`}>
            <span className={prefix}>{line}</span>
          </div>
        );
      })}
    </div>
  );
}

export default function DiffView() {
  const diff = mockDiff;

  return (
    <div className="p-2 space-y-3 animate-fade-in">
      {/* 文件列表 */}
      <div className="space-y-2">
        <div className="border border-surface-border rounded-lg overflow-hidden bg-surface-base">
          {/* 文件头 */}
          <div className="flex items-center justify-between px-3 py-2 bg-surface-panel border-b border-surface-border">
            <span className="text-xs font-medium text-surface-text">{diff.file}</span>
            <span className="text-[10px] text-diff-add mr-1">+6</span>
            <span className="text-[10px] text-diff-remove">-3</span>
          </div>

          {/* Diff 内容 */}
          <div className="overflow-auto max-h-80">
            <HunkBlock hunk={diff.hunks[0]} />
          </div>

          {/* 操作按钮 */}
          {diff.status === "pending" && (
            <div className="flex gap-2 px-3 py-2 bg-surface-panel border-t border-surface-border">
              <button className="flex-1 px-2 py-1 text-xs bg-diff-add/20 text-diff-add border border-diff-add/40 rounded hover:bg-diff-add/30 transition-colors">
                ✓ Apply
              </button>
              <button className="flex-1 px-2 py-1 text-xs bg-diff-remove/20 text-diff-remove border border-diff-remove/40 rounded hover:bg-diff-remove/30 transition-colors">
                ✕ Reject
              </button>
            </div>
          )}
          {diff.status === "applied" && (
            <div className="px-3 py-2 bg-diff-add/10 text-diff-add text-xs text-center">
              ✓ Applied
            </div>
          )}
          {diff.status === "rejected" && (
            <div className="px-3 py-2 bg-diff-remove/10 text-diff-remove text-xs text-center">
              ✕ Rejected
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
