import { useEffect, useState } from "react";
import { Check, GitBranch, History, Pencil, Plus, RefreshCw, Trash2, X } from "lucide-react";
import { useAgentStore } from "../../stores/useAgentStore";
import { isTauriRuntime } from "../../utils/tauri";
import { useT } from "../../i18n";
import type { MessageKey } from "../../i18n/messages";


/** 手打的任务名上限，和后端 `session_title_from` 的截断点一致：界面要让这条边界看得见 */
const MAX_TASK_NAME_CHARS = 60;


/**
 * 历史任务面板：新开一个任务，或回到之前某一次。
 *
 * 界面上统一叫 **task**（代码里叫 session，见 `AgentPanel` 顶部的说明）：用户问的是
 * "怎么新开 task、怎么看历史 task"，而这里就是那两件事唯一的入口。
 *
 * 措辞上刻意反复说"只回来上下文"：一个任务在这里就是那几轮对话，计划不恢复（diff 描述的是
 * 磁盘某一刻的样子，而审查区里那些待审查改动是真实存在的，换任务不动它们）。让用户以为改动
 * 也一起换了，正是这个产品最该避免的那种误解。
 */

export default function SessionHistory() {
  const t = useT();
  const sessions = useAgentStore((s) => s.sessions);
  const activeSessionId = useAgentStore((s) => s.activeSessionId);
  const sessionWarning = useAgentStore((s) => s.sessionWarning);
  const sessionsAreSaved = useAgentStore((s) => s.sessionsAreSaved);
  const loadSessions = useAgentStore((s) => s.loadSessions);
  const startNewSession = useAgentStore((s) => s.startNewSession);
  const resumeSession = useAgentStore((s) => s.resumeSession);
  const deleteSession = useAgentStore((s) => s.deleteSession);
  const renameSession = useAgentStore((s) => s.renameSession);
  const forkSession = useAgentStore((s) => s.forkSession);
  const isStreaming = useAgentStore((s) => s.isStreaming);
  const [error, setError] = useState<string | null>(null);
  // 正在改名的那一行，以及输入到一半的名字。只有一行能处于改名状态：两行同时改的话，
  // 保存按钮点的是哪一行只能靠猜。
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [draftName, setDraftName] = useState("");

  // 挂载时读一次，之后每次 streaming 落下沿再读：刚跑完的那一轮此刻才进历史。
  // 挂载那一次不看 `isStreaming` —— 面板在运行途中打开时也必须列出已有的会话，
  // 否则它会显示"还没有历史会话"，而磁盘上明明有。
  useEffect(() => {
    void loadSessions();
  }, [loadSessions]);
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
          <span className="truncate">{t("session.header")}</span>
        </div>
        <div className="flex flex-shrink-0 items-center gap-1">
          <button
            type="button"
            onClick={() => void run(loadSessions)}
            aria-label={t("session.refresh")}
            title={t("session.refresh.title")}
            className="rounded p-1 text-surface-muted hover:bg-surface-border/30 hover:text-surface-text"
          >
            <RefreshCw aria-hidden="true" className="h-3 w-3" />
          </button>
          <button
            type="button"
            onClick={() => void run(startNewSession)}
            title={t("session.new.title")}
            data-testid="session-new"
            className="flex items-center gap-1 rounded border border-surface-border px-1.5 py-0.5 text-[10px] text-surface-text hover:bg-surface-border/30"
          >
            <Plus aria-hidden="true" className="h-3 w-3" />
            {t("session.new")}
          </button>
        </div>

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
          <p className="px-3 py-3 text-[11px] text-surface-muted">{t(emptyStateKey(sessionsAreSaved))}</p>
        ) : (
          <ul className="divide-y divide-surface-border/60">
            {sessions.map((session) => {
              const active = session.id === activeSessionId;
              const renaming = renamingId === session.id;
              const age = relativeTime(session.updatedAt);
              return (
                <li key={session.id} className="px-3 py-2">
                  {renaming ? (
                    <form
                      className="flex items-center gap-1.5"
                      onSubmit={(event) => {
                        event.preventDefault();
                        void run(async () => {
                          await renameSession(session.id, draftName);
                          setRenamingId(null);
                        });
                      }}
                    >
                      <input
                        autoFocus
                        value={draftName}
                        maxLength={MAX_TASK_NAME_CHARS}
                        onChange={(event) => setDraftName(event.target.value)}
                        onKeyDown={(event) => {
                          if (event.key === "Escape") {
                            event.preventDefault();
                            setRenamingId(null);
                          }
                        }}
                        aria-label={t("session.rename", { name: session.title })}
                        className="min-w-0 flex-1 rounded border border-surface-border bg-surface-base px-2 py-1 text-[11px] text-surface-text"
                      />
                      <button
                        type="submit"
                        // 空名字不提交：后端会拒，而"提交了一个空名字"在界面上看起来像成功
                        disabled={!draftName.trim()}
                        aria-label={t("session.rename.save")}
                        className="rounded p-1 text-surface-muted hover:bg-surface-border/30 hover:text-accent-blue disabled:opacity-40"
                      >
                        <Check aria-hidden="true" className="h-3 w-3" />
                      </button>
                      <button
                        type="button"
                        onClick={() => setRenamingId(null)}
                        aria-label={t("session.rename.cancel")}
                        className="rounded p-1 text-surface-muted hover:bg-surface-border/30"
                      >
                        <X aria-hidden="true" className="h-3 w-3" />
                      </button>
                    </form>
                  ) : (
                  <div className="flex items-start justify-between gap-2">
                    <button
                      type="button"
                      onClick={() => void run(() => resumeSession(session.id))}
                      disabled={active}
                      title={active ? t("session.resume.active") : t("session.resume.title")}

                      className={`min-w-0 flex-1 text-left ${active ? "cursor-default" : "hover:text-accent-blue"}`}
                    >
                      <div className="flex items-center gap-1.5">
                        <span className="truncate text-[11px] text-surface-text">{session.title}</span>
                        {active && (
                          <span className="flex-shrink-0 rounded bg-accent-blue/15 px-1 py-0.5 text-[9px] leading-none text-accent-blue">
                            {t("session.current")}
                          </span>
                        )}
                      </div>
                      <div className="mt-0.5 font-mono text-[10px] text-surface-muted">
                        {t(age.key, { count: age.count })} ·{" "}
                        {t(session.turnCount === 1 ? "session.turns.one" : "session.turns.many", {
                          count: session.turnCount,
                        })}
                      </div>
                      {session.lastOutcome && (
                        <div className="mt-0.5 truncate text-[10px] text-surface-muted">
                          {session.lastOutcome}
                        </div>
                      )}
                    </button>
                    <div className="flex flex-shrink-0 items-center gap-0.5">
                      <button
                        type="button"
                        onClick={() => {
                          setRenamingId(session.id);
                          setDraftName(session.title);
                        }}
                        aria-label={t("session.rename", { name: session.title })}
                        title={t("session.rename.title")}
                        className="rounded p-1 text-surface-muted hover:bg-surface-border/30 hover:text-surface-text"
                      >
                        <Pencil aria-hidden="true" className="h-3 w-3" />
                      </button>
                      <button
                        type="button"
                        onClick={() => void run(() => forkSession(session.id))}
                        aria-label={t("session.fork", { name: session.title })}
                        title={t("session.fork.title")}
                        className="rounded p-1 text-surface-muted hover:bg-surface-border/30 hover:text-accent-blue"
                      >
                        <GitBranch aria-hidden="true" className="h-3 w-3" />
                      </button>
                      <button
                        type="button"
                        onClick={() => void run(() => deleteSession(session.id))}
                        aria-label={t("session.delete", { name: session.title })}
                        title={t("session.delete.title")}
                        className="rounded p-1 text-surface-muted hover:bg-surface-border/30 hover:text-diff-delete"
                      >
                        <Trash2 aria-hidden="true" className="h-3 w-3" />
                      </button>
                    </div>
                  </div>
                  )}
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
 * 空列表要说清是哪一种空。
 *
 * "还没聊过"和"这个环境根本不保存"在屏幕上长得一模一样，而后者意味着用户刚才那一问不会被
 * 记住 —— 不说清就等于让他以为存好了。任务是按工作区分组的，没打开工作区时一条都存不下来。
 *
 * 返回文案键而不是成句：这个判断和语言无关，塞进 `t` 就等于把它绑在组件渲染上，测不了。
 */
export function emptyStateKey(sessionsAreSaved: boolean): MessageKey {
  if (!isTauriRuntime()) {
    return "session.empty.noBackend";
  }
  if (!sessionsAreSaved) {
    return "session.empty.noWorkspace";
  }
  return "session.empty.none";
}


/**
 * "多久以前"。
 *
 * 显示相对时间而不是绝对时间戳：认出"是不是刚才那次"靠的是间隔，而不是 14:32 这个数字。
 * 0 是"后端没给时间戳"（见 `normalizeAgentSessionList`），这时不能算成 1970 年。
 *
 * 返回键 + 数字，不返回拼好的句子：中英文的量词位置不同（`5m ago` / `5 分钟前`），
 * 在这里拼串就等于把英文语序写死进逻辑。
 */
export function relativeTime(
  updatedAt: number,
  now: number = Date.now()
): { key: MessageKey; count: number } {
  if (!Number.isFinite(updatedAt) || updatedAt <= 0) return { key: "session.time.unknown", count: 0 };
  const seconds = Math.floor((now - updatedAt) / 1000);
  if (seconds < 60) return { key: "session.time.now", count: 0 };
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return { key: "session.time.minutes", count: minutes };
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return { key: "session.time.hours", count: hours };
  return { key: "session.time.days", count: Math.floor(hours / 24) };
}

