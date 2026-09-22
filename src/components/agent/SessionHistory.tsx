import { useEffect, useState } from "react";
import { History, Plus, Trash2 } from "lucide-react";
import { useAgentStore } from "../../stores/useAgentStore";

/**
 * 历史会话面板：新建一个会话，或回到之前某一次对话。
 *
 * 在这之前这两件事在界面上**根本没有入口** —— 唯一沾边的是恢复横幅里那个 Clear 按钮，
 * 而它只在有恢复出来的步骤时才出现。所以用户找不到"新建会话"和"历史会话"不是猜不到位置，
 * 是它们确实不存在。
 *
 * 措辞上刻意反复说"只回来上下文"：一个会话在这里就是那几轮对话，diff 和计划不恢复（diff
 * 描述的是磁盘某一刻的样子，隔天多半已经对不上）。让用户以为改动也一起回来了，正是这个
 * 产品最该避免的那种误解。
 */
export default function SessionHistory() {
  const sessions = useAgentStore((s) => s.sessions);
  const activeSessionId = useAgentStore((s) => s.activeSessionId);
  const sessionWarning = useAgentStore((s) => s.sessionWarning);
  const loadSessions = useAgentStore((s) => s.loadSessions);
  const startNewSession = useAgentStore((s) => s.startNewSession);
  const resumeSession = useAgentStore((s) => s.resumeSession);
  const deleteSession = useAgentStore((s) => s.deleteSession);
  const isStreaming = useAgentStore((s) => s.isStreaming);
  const [error, setError] = useState<string | null>(null);

  // 打开面板就读一次，运行结束（streaming 落下沿）再读一次：刚跑完的那一轮此刻才进历史
  useEffect(() => {
    if (!isStreaming) void loadSessions();
  }, [isStreaming, loadSessions]);

  const run = async (action: () => Promise<void>) => {
    setError(null);
    try {
      await action();
    } catch (err) {
      // 运行中换会话会被后端拒绝。说出来，而不是让按钮看起来没反应。
      setError(String(err));
    }
  };

  return (
    <div data-testid="agent-sessions" className="flex h-full flex-col">
      <div className="flex items-center justify-between gap-2 border-b border-surface-border px-3 py-2">
        <div className="flex min-w-0 items-center gap-1.5 text-[11px] text-surface-muted">
          <History aria-hidden="true" className="h-3.5 w-3.5 flex-shrink-0" />
          <span className="truncate">Sessions in this workspace</span>
        </div>
        <button
          type="button"
          onClick={() => void run(startNewSession)}
          title="Start a new session. The current conversation stays in this list."
          data-testid="session-new"
          className="flex items-center gap-1 rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-text hover:bg-surface-border/30"
        >
          <Plus aria-hidden="true" className="h-3 w-3" />
          New session
        </button>
      </div>

      {sessionWarning && (
        <div className="border-b border-diff-modify/30 bg-diff-modify/10 px-3 py-1.5 text-[10px] text-diff-modify">
          {sessionWarning}
        </div>
      )}
      {error && (
        <div className="border-b border-diff-delete/40 bg-diff-delete/10 px-3 py-1.5 text-[10px] text-diff-delete">
          {error}
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-auto">
        {sessions.length === 0 ? (
          <p className="px-3 py-3 text-[11px] text-surface-muted">
            No saved sessions yet. A session is saved once you send the first prompt.
          </p>
        ) : (
          <ul className="divide-y divide-surface-border/60">
            {sessions.map((session) => {
              const active = session.id === activeSessionId;
              return (
                <li key={session.id} className="px-3 py-2">
                  <div className="flex items-start justify-between gap-2">
                    <button
                      type="button"
                      onClick={() => void run(() => resumeSession(session.id))}
                      disabled={active}
                      title={
                        active
                          ? "This is the session you are in"
                          : "Load this session's conversation back into the model context. File changes and the plan are not restored."
                      }
                      className={`min-w-0 flex-1 text-left ${active ? "cursor-default" : "hover:text-accent-blue"}`}
                    >
                      <div className="flex items-center gap-1.5">
                        <span className="truncate text-[11px] text-surface-text">{session.title}</span>
                        {active && (
                          <span className="flex-shrink-0 rounded bg-accent-blue/15 px-1 py-0.5 text-[9px] leading-none text-accent-blue">
                            current
                          </span>
                        )}
                      </div>
                      <div className="mt-0.5 font-mono text-[10px] text-surface-muted">
                        {formatRelativeTime(session.updatedAt)} · {session.turnCount} turn
                        {session.turnCount === 1 ? "" : "s"}
                      </div>
                      {session.lastOutcome && (
                        <div className="mt-0.5 truncate text-[10px] text-surface-muted">
                          {session.lastOutcome}
                        </div>
                      )}
                    </button>
                    <button
                      type="button"
                      onClick={() => void run(() => deleteSession(session.id))}
                      aria-label={`Delete session ${session.title}`}
                      title="Delete this session's saved conversation"
                      className="flex-shrink-0 rounded p-1 text-surface-muted hover:bg-surface-border/30 hover:text-diff-delete"
                    >
                      <Trash2 aria-hidden="true" className="h-3 w-3" />
                    </button>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}

/**
 * "多久以前"。
 *
 * 显示相对时间而不是绝对时间戳：认出"是不是刚才那次"靠的是间隔，而不是 14:32 这个数字。
 * 0 是"后端没给时间戳"（见 `normalizeAgentSessionList`），这时不能算成 1970 年。
 */
export function formatRelativeTime(updatedAt: number, now: number = Date.now()): string {
  if (!Number.isFinite(updatedAt) || updatedAt <= 0) return "unknown time";
  const seconds = Math.floor((now - updatedAt) / 1000);
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}
