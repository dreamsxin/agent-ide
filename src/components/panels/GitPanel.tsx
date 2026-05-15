import { useState, useEffect, useCallback, useMemo } from "react";
import { useGitStore } from "../../stores/useGitStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useLogStore } from "../../stores/useLogStore";
import type { GitDiffKind, GitStatusEntry } from "../../types/project";

const STATUS_ICONS: Record<string, { icon: string; color: string }> = {
  modified: { icon: "M", color: "text-diff-modify" },
  added: { icon: "A", color: "text-accent-green" },
  deleted: { icon: "D", color: "text-diff-remove" },
  untracked: { icon: "U", color: "text-accent-green" },
  renamed: { icon: "R", color: "text-accent-blue" },
  conflicted: { icon: "!", color: "text-diff-remove" },
};

const DIFF_LABELS: Record<GitDiffKind, string> = {
  worktree: "Worktree",
  staged: "Staged",
  all: "All",
};

function entryKey(entry: GitStatusEntry): string {
  return `${entry.staged ? "staged" : "worktree"}:${entry.path}`;
}

function uniquePaths(entries: GitStatusEntry[]): string[] {
  return Array.from(new Set(entries.map((entry) => entry.path)));
}

export default function GitPanel() {
  const workspacePath = useLayoutStore((s) => s.workspacePath);
  const status = useGitStore((s) => s.status);
  const diff = useGitStore((s) => s.diff);
  const loading = useGitStore((s) => s.loading);
  const error = useGitStore((s) => s.error);
  const fetchStatus = useGitStore((s) => s.fetchStatus);
  const fetchDiff = useGitStore((s) => s.fetchDiff);
  const clearDiff = useGitStore((s) => s.clearDiff);
  const commit = useGitStore((s) => s.commit);
  const stageFiles = useGitStore((s) => s.stageFiles);
  const unstageFiles = useGitStore((s) => s.unstageFiles);
  const discardFiles = useGitStore((s) => s.discardFiles);
  const checkoutBranch = useGitStore((s) => s.checkoutBranch);
  const checkoutRemoteBranch = useGitStore((s) => s.checkoutRemoteBranch);
  const fetchRemote = useGitStore((s) => s.fetch);
  const pullRemote = useGitStore((s) => s.pull);
  const pushRemote = useGitStore((s) => s.push);
  const resolveConflict = useGitStore((s) => s.resolveConflict);
  const addLog = useLogStore((s) => s.addLog);

  const projectPath = workspacePath || ".";
  const [message, setMessage] = useState("");
  const [selectedEntryKey, setSelectedEntryKey] = useState<string | null>(null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [selectedKeys, setSelectedKeys] = useState<string[]>([]);
  const [diffKind, setDiffKind] = useState<GitDiffKind>("all");
  const [branchName, setBranchName] = useState("");
  const [remoteBranch, setRemoteBranch] = useState("");
  const [credentialUsername, setCredentialUsername] = useState("");
  const [credentialPassword, setCredentialPassword] = useState("");
  const [menu, setMenu] = useState<{
    x: number;
    y: number;
    entry: GitStatusEntry;
  } | null>(null);

  useEffect(() => {
    fetchStatus(projectPath);
  }, [fetchStatus, projectPath]);

  const staged = useMemo(() => status?.entries.filter((entry) => entry.staged) ?? [], [status]);
  const unstaged = useMemo(() => status?.entries.filter((entry) => !entry.staged) ?? [], [status]);
  const allEntries = useMemo(() => [...staged, ...unstaged], [staged, unstaged]);
  const selectedEntries = useMemo(
    () => allEntries.filter((entry) => selectedKeys.includes(entryKey(entry))),
    [allEntries, selectedKeys]
  );
  const selectedStaged = selectedEntries.filter((entry) => entry.staged);
  const selectedUnstaged = selectedEntries.filter((entry) => !entry.staged);
  const localBranches = status?.branches.filter((branch) => !branch.remote) ?? [];
  const remoteBranches = status?.branches.filter((branch) => branch.remote) ?? [];
  const credentials =
    credentialUsername.trim() || credentialPassword.trim()
      ? {
          username: credentialUsername.trim(),
          password: credentialPassword,
        }
      : null;

  useEffect(() => {
    if (!selectedEntryKey) return;
    if (!allEntries.some((entry) => entryKey(entry) === selectedEntryKey)) {
      setSelectedEntryKey(null);
      setSelectedFile(null);
      clearDiff();
    }
  }, [allEntries, clearDiff, selectedEntryKey]);

  const handleRefresh = useCallback(() => {
    fetchStatus(projectPath);
    addLog({
      time: new Date().toLocaleTimeString(),
      level: "info",
      source: "git",
      message: "Git status refreshed",
    });
  }, [fetchStatus, projectPath, addLog]);

  const previewEntry = useCallback(
    (entry: GitStatusEntry, kind: GitDiffKind = entry.staged ? "staged" : "worktree") => {
      setSelectedEntryKey(entryKey(entry));
      setSelectedFile(entry.path);
      setDiffKind(kind);
      fetchDiff(projectPath, entry.path, kind);
    },
    [fetchDiff, projectPath]
  );

  const handleDiffKindChange = useCallback(
    (kind: GitDiffKind) => {
      if (!selectedFile) return;
      setDiffKind(kind);
      fetchDiff(projectPath, selectedFile, kind);
    },
    [fetchDiff, projectPath, selectedFile]
  );

  const toggleSelection = useCallback((entry: GitStatusEntry) => {
    const key = entryKey(entry);
    setSelectedKeys((current) =>
      current.includes(key) ? current.filter((item) => item !== key) : [...current, key]
    );
  }, []);

  const refreshAfterAction = useCallback(
    async (logMessage: string) => {
      addLog({
        time: new Date().toLocaleTimeString(),
        level: "success",
        source: "git",
        message: logMessage,
      });
      await fetchStatus(projectPath);
      clearDiff();
      setSelectedEntryKey(null);
      setSelectedFile(null);
      setSelectedKeys([]);
      setMenu(null);
    },
    [addLog, fetchStatus, projectPath, clearDiff]
  );

  const handleCommit = useCallback(async () => {
    if (!message.trim()) return;

    const oid = await commit(projectPath, message.trim());
    if (oid) {
      addLog({
        time: new Date().toLocaleTimeString(),
        level: "success",
        source: "git",
        message: `Committed: ${oid.slice(0, 7)} - ${message}`,
      });
      setMessage("");
      await fetchStatus(projectPath);
      clearDiff();
      setSelectedEntryKey(null);
      setSelectedFile(null);
      setSelectedKeys([]);
    }
  }, [message, commit, projectPath, addLog, fetchStatus, clearDiff]);

  const handleCheckoutBranch = useCallback(
    async (branch: string) => {
      if (!branch || branch === status?.branch) return;
      if (await checkoutBranch(projectPath, branch, false)) {
        addLog({
          time: new Date().toLocaleTimeString(),
          level: "success",
          source: "git",
          message: `Checked out ${branch}`,
        });
        await fetchStatus(projectPath);
        clearDiff();
        setSelectedEntryKey(null);
        setSelectedFile(null);
      }
    },
    [addLog, checkoutBranch, clearDiff, fetchStatus, projectPath, status?.branch]
  );

  const handleCreateBranch = useCallback(async () => {
    const name = branchName.trim();
    if (!name) return;
    if (await checkoutBranch(projectPath, name, true)) {
      addLog({
        time: new Date().toLocaleTimeString(),
        level: "success",
        source: "git",
        message: `Created and checked out ${name}`,
      });
      setBranchName("");
      await fetchStatus(projectPath);
      clearDiff();
    }
  }, [addLog, branchName, checkoutBranch, clearDiff, fetchStatus, projectPath]);

  const handleCheckoutRemoteBranch = useCallback(async () => {
    if (!remoteBranch) return;
    if (await checkoutRemoteBranch(projectPath, remoteBranch)) {
      addLog({
        time: new Date().toLocaleTimeString(),
        level: "success",
        source: "git",
        message: `Checked out tracking branch ${remoteBranch}`,
      });
      setRemoteBranch("");
      await fetchStatus(projectPath);
      clearDiff();
    }
  }, [addLog, checkoutRemoteBranch, clearDiff, fetchStatus, projectPath, remoteBranch]);

  const handleRemoteAction = useCallback(
    async (kind: "fetch" | "pull" | "push") => {
      const action = kind === "fetch" ? fetchRemote : kind === "pull" ? pullRemote : pushRemote;
      if (await action(projectPath, undefined, credentials)) {
        addLog({
          time: new Date().toLocaleTimeString(),
          level: "success",
          source: "git",
          message: `Git ${kind} completed`,
        });
        await fetchStatus(projectPath);
        clearDiff();
      }
    },
    [addLog, clearDiff, credentials, fetchRemote, fetchStatus, projectPath, pullRemote, pushRemote]
  );

  const handleResolveConflict = useCallback(
    async (file: string, resolution: "current" | "incoming" | "both") => {
      if (await resolveConflict(projectPath, file, resolution)) {
        addLog({
          time: new Date().toLocaleTimeString(),
          level: "success",
          source: "git",
          message: `Resolved ${file} with ${resolution}`,
        });
        await fetchStatus(projectPath);
        clearDiff();
        setSelectedFile(null);
        setSelectedEntryKey(null);
      }
    },
    [addLog, clearDiff, fetchStatus, projectPath, resolveConflict]
  );

  const handleConflictDiff = useCallback(
    (file: string) => {
      const entry = allEntries.find((item) => item.path === file) ?? {
        path: file,
        status: "conflicted" as const,
        old_path: null,
        staged: false,
      };
      previewEntry(entry, "all");
    },
    [allEntries, previewEntry]
  );

  const handleStage = useCallback(
    async (entries: GitStatusEntry[]) => {
      const files = uniquePaths(entries.filter((entry) => !entry.staged));
      if (files.length === 0) return;
      if (await stageFiles(projectPath, files)) {
        await refreshAfterAction(`Staged ${files.length === 1 ? files[0] : `${files.length} files`}`);
      }
    },
    [stageFiles, projectPath, refreshAfterAction]
  );

  const handleUnstage = useCallback(
    async (entries: GitStatusEntry[]) => {
      const files = uniquePaths(entries.filter((entry) => entry.staged));
      if (files.length === 0) return;
      if (await unstageFiles(projectPath, files)) {
        await refreshAfterAction(`Unstaged ${files.length === 1 ? files[0] : `${files.length} files`}`);
      }
    },
    [unstageFiles, projectPath, refreshAfterAction]
  );

  const handleDiscard = useCallback(
    async (entries: GitStatusEntry[]) => {
      const files = uniquePaths(entries);
      if (files.length === 0) return;
      const label = files.length === 1 ? files[0] : `${files.length} files`;
      const ok = window.confirm(`Discard changes in ${label}? This cannot be undone.`);
      if (!ok) return;
      if (await discardFiles(projectPath, files)) {
        await refreshAfterAction(`Discarded ${label}`);
      }
    },
    [discardFiles, projectPath, refreshAfterAction]
  );

  const handleContextMenu = useCallback((event: React.MouseEvent, entry: GitStatusEntry) => {
    event.preventDefault();
    setMenu({ x: event.clientX, y: event.clientY, entry });
  }, []);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("keydown", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", close);
      window.removeEventListener("blur", close);
    };
  }, [menu]);

  const renderEntry = (entry: GitStatusEntry) => {
    const info = STATUS_ICONS[entry.status] ?? { icon: "?", color: "text-surface-muted" };
    const key = entryKey(entry);
    const isPreviewed = selectedEntryKey === key;
    const isChecked = selectedKeys.includes(key);
    return (
      <div
        key={`${key}-${entry.status}`}
        onClick={() => previewEntry(entry)}
        onContextMenu={(event) => handleContextMenu(event, entry)}
        className={`group flex items-center gap-1.5 px-3 py-1 cursor-pointer transition-colors ${
          isPreviewed
            ? "bg-accent-blue/10 text-surface-text"
            : "text-surface-text hover:bg-surface-border/20"
        }`}
      >
        <input
          type="checkbox"
          checked={isChecked}
          onChange={() => toggleSelection(entry)}
          onClick={(event) => event.stopPropagation()}
          className="h-3 w-3 accent-accent-blue"
          title="Select for batch action"
        />
        <span className={`w-4 text-center font-bold text-[10px] ${info.color}`}>{info.icon}</span>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px]">{entry.path}</span>
        <button
          onClick={(event) => {
            event.stopPropagation();
            entry.staged ? handleUnstage([entry]) : handleStage([entry]);
          }}
          title={entry.staged ? "Unstage" : "Stage"}
          className="opacity-0 group-hover:opacity-100 text-surface-muted hover:text-surface-text px-1"
        >
          {entry.staged ? "-" : "+"}
        </button>
      </div>
    );
  };

  return (
    <div className="h-full flex flex-col bg-surface-panel text-xs">
      <div className="flex items-center justify-between px-3 py-2 border-b border-surface-border">
        <span className="font-semibold text-surface-text tracking-wide text-[11px]">
          SOURCE CONTROL
        </span>
        <button
          onClick={handleRefresh}
          className="text-surface-muted hover:text-surface-text text-sm leading-none px-1"
          title="Refresh"
        >
          ↻
        </button>
      </div>

      {error && (
        <div className="px-3 py-2 text-diff-remove text-[10px] border-b border-surface-border">
          {error}
        </div>
      )}

      {status && (
        <div className="px-3 py-2 border-b border-surface-border">
          <div className="flex items-center gap-1.5 text-surface-text">
            <span className="text-accent-blue">⎇</span>
            <select
              value={status.branch}
              onChange={(event) => handleCheckoutBranch(event.target.value)}
              disabled={loading}
              className="min-w-0 flex-1 bg-surface-base border border-surface-border rounded px-1.5 py-1 font-mono text-[11px] text-surface-text focus:outline-none focus:border-accent-blue"
              title="Checkout branch"
            >
              {localBranches.map((branch) => (
                <option key={branch.name} value={branch.name}>
                  {branch.name}
                </option>
              ))}
            </select>
          </div>
          {status.upstream && (
            <div className="mt-1 truncate text-[10px] text-surface-muted" title={status.upstream}>
              upstream: {status.upstream}
            </div>
          )}
          {status.ahead > 0 && <div className="text-accent-blue text-[10px] mt-0.5">↑{status.ahead} ahead</div>}
          {status.behind > 0 && <div className="text-diff-modify text-[10px] mt-0.5">↓{status.behind} behind</div>}
          <div className="mt-2 flex gap-1">
            <button
              onClick={() => handleRemoteAction("fetch")}
              disabled={loading}
              className="rounded border border-surface-border px-2 py-1 text-[10px] text-surface-text hover:bg-surface-border/30 disabled:opacity-40"
            >
              Fetch
            </button>
            <button
              onClick={() => handleRemoteAction("pull")}
              disabled={loading}
              className="rounded border border-surface-border px-2 py-1 text-[10px] text-surface-text hover:bg-surface-border/30 disabled:opacity-40"
            >
              Pull
            </button>
            <button
              onClick={() => handleRemoteAction("push")}
              disabled={loading}
              className="rounded border border-surface-border px-2 py-1 text-[10px] text-surface-text hover:bg-surface-border/30 disabled:opacity-40"
            >
              Push
            </button>
          </div>
          <div className="mt-2 grid grid-cols-2 gap-1">
            <input
              value={credentialUsername}
              onChange={(event) => setCredentialUsername(event.target.value)}
              placeholder="git username"
              className="min-w-0 bg-surface-base border border-surface-border rounded px-2 py-1 text-[10px] text-surface-text focus:outline-none focus:border-accent-blue placeholder:text-surface-muted"
            />
            <input
              value={credentialPassword}
              onChange={(event) => setCredentialPassword(event.target.value)}
              type="password"
              placeholder="token/password"
              className="min-w-0 bg-surface-base border border-surface-border rounded px-2 py-1 text-[10px] text-surface-text focus:outline-none focus:border-accent-blue placeholder:text-surface-muted"
            />
          </div>
          <div className="mt-2 flex gap-1">
            <input
              value={branchName}
              onChange={(event) => setBranchName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  handleCreateBranch();
                }
              }}
              placeholder="new branch"
              className="min-w-0 flex-1 bg-surface-base border border-surface-border rounded px-2 py-1 text-[10px] text-surface-text focus:outline-none focus:border-accent-blue placeholder:text-surface-muted"
            />
            <button
              onClick={handleCreateBranch}
              disabled={!branchName.trim() || loading}
              className="rounded border border-surface-border px-2 py-1 text-[10px] text-surface-text hover:bg-surface-border/30 disabled:opacity-40"
            >
              Create
            </button>
          </div>
          {remoteBranches.length > 0 && (
            <div className="mt-2 flex gap-1">
              <select
                value={remoteBranch}
                onChange={(event) => setRemoteBranch(event.target.value)}
                disabled={loading}
                className="min-w-0 flex-1 bg-surface-base border border-surface-border rounded px-1.5 py-1 font-mono text-[10px] text-surface-text focus:outline-none focus:border-accent-blue"
                title="Remote branch"
              >
                <option value="">remote branch...</option>
                {remoteBranches.map((branch) => (
                  <option key={branch.name} value={branch.name}>
                    {branch.name}
                  </option>
                ))}
              </select>
              <button
                onClick={handleCheckoutRemoteBranch}
                disabled={!remoteBranch || loading}
                className="rounded border border-surface-border px-2 py-1 text-[10px] text-surface-text hover:bg-surface-border/30 disabled:opacity-40"
              >
                Track
              </button>
            </div>
          )}
          {status.conflicts.length > 0 && (
            <div className="mt-2 rounded border border-diff-remove/40 bg-diff-remove/10 p-2 text-[10px] text-diff-remove">
              <div className="mb-1 font-semibold">
                {status.conflicts.length} conflict{status.conflicts.length === 1 ? "" : "s"} detected
              </div>
              <div className="space-y-1">
                {status.conflicts.map((file) => (
                  <div key={file} className="rounded border border-diff-remove/20 bg-surface-panel/70 p-1">
                    <button
                      onClick={() => handleConflictDiff(file)}
                      className="block w-full truncate text-left font-mono text-diff-remove hover:underline"
                      title="Open conflict diff"
                    >
                      {file}
                    </button>
                    <div className="mt-1 flex flex-wrap gap-1">
                      <button
                        onClick={() => handleResolveConflict(file, "current")}
                        className="rounded border border-surface-border px-1.5 py-0.5 text-surface-text hover:bg-surface-border/30"
                      >
                        Current
                      </button>
                      <button
                        onClick={() => handleResolveConflict(file, "incoming")}
                        className="rounded border border-surface-border px-1.5 py-0.5 text-surface-text hover:bg-surface-border/30"
                      >
                        Incoming
                      </button>
                      <button
                        onClick={() => handleResolveConflict(file, "both")}
                        className="rounded border border-surface-border px-1.5 py-0.5 text-surface-text hover:bg-surface-border/30"
                      >
                        Both
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      {selectedKeys.length > 0 && (
        <div className="border-b border-surface-border px-2 py-2">
          <div className="mb-1.5 text-[10px] uppercase tracking-wider text-surface-muted">
            {selectedKeys.length} selected
          </div>
          <div className="flex flex-wrap gap-1">
            <button
              onClick={() => handleStage(selectedEntries)}
              disabled={selectedUnstaged.length === 0 || loading}
              className="rounded border border-surface-border px-2 py-1 text-[10px] text-surface-text hover:bg-surface-border/30 disabled:opacity-40"
            >
              Stage
            </button>
            <button
              onClick={() => handleUnstage(selectedEntries)}
              disabled={selectedStaged.length === 0 || loading}
              className="rounded border border-surface-border px-2 py-1 text-[10px] text-surface-text hover:bg-surface-border/30 disabled:opacity-40"
            >
              Unstage
            </button>
            <button
              onClick={() => handleDiscard(selectedEntries)}
              disabled={selectedEntries.length === 0 || loading}
              className="rounded border border-diff-remove/40 px-2 py-1 text-[10px] text-diff-remove hover:bg-diff-remove/10 disabled:opacity-40"
            >
              Discard
            </button>
            <button
              onClick={() => setSelectedKeys([])}
              className="rounded border border-surface-border px-2 py-1 text-[10px] text-surface-muted hover:text-surface-text hover:bg-surface-border/30"
            >
              Clear
            </button>
          </div>
        </div>
      )}

      {loading && <div className="px-3 py-2 text-surface-muted animate-pulse">Loading...</div>}

      <div className="min-h-0 flex-1 overflow-auto">
        {staged.length > 0 && (
          <div>
            <div className="px-3 py-1.5 text-surface-muted font-semibold text-[10px] uppercase tracking-wider">
              Staged Changes ({staged.length})
            </div>
            {staged.map(renderEntry)}
          </div>
        )}

        {unstaged.length > 0 && (
          <div>
            <div className="px-3 py-1.5 text-surface-muted font-semibold text-[10px] uppercase tracking-wider">
              Changes ({unstaged.length})
            </div>
            {unstaged.map(renderEntry)}
          </div>
        )}

        {status && staged.length === 0 && unstaged.length === 0 && (
          <div className="px-3 py-4 text-surface-muted text-center">No changes detected.</div>
        )}

        {!status && !loading && !error && (
          <div className="px-3 py-4 text-surface-muted text-center">No git repository found.</div>
        )}
      </div>

      {diff && selectedFile && (
        <div className="border-t border-surface-border max-h-60 overflow-auto">
          <div className="sticky top-0 bg-surface-panel border-b border-surface-border px-3 py-1.5">
            <div className="mb-1 truncate text-surface-muted font-semibold text-[10px] uppercase">
              Diff: {selectedFile}
            </div>
            <div className="flex gap-1">
              {(["worktree", "staged", "all"] as GitDiffKind[]).map((kind) => (
                <button
                  key={kind}
                  onClick={() => handleDiffKindChange(kind)}
                  className={`rounded border px-2 py-0.5 text-[10px] ${
                    diffKind === kind
                      ? "border-accent-blue bg-accent-blue/10 text-surface-text"
                      : "border-surface-border text-surface-muted hover:text-surface-text"
                  }`}
                >
                  {DIFF_LABELS[kind]}
                </button>
              ))}
            </div>
          </div>
          <pre className="px-3 pb-2 font-mono text-[10px] text-surface-text whitespace-pre-wrap leading-relaxed">
            {diff.split("\n").map((line, i) => {
              let color = "text-surface-text";
              if (line.startsWith("+")) color = "text-diff-add";
              else if (line.startsWith("-")) color = "text-diff-remove";
              else if (line.startsWith("@@")) color = "text-accent-blue";
              else if (line.startsWith("diff") || line.startsWith("---") || line.startsWith("+++")) color = "text-surface-muted";

              return (
                <div key={i} className={color}>
                  {line}
                </div>
              );
            })}
          </pre>
        </div>
      )}

      {status && staged.length > 0 && (
        <div className="border-t border-surface-border p-2">
          <textarea
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            placeholder="Commit message..."
            rows={2}
            className="w-full bg-surface-base border border-surface-border rounded px-2 py-1 text-surface-text text-[11px] font-mono resize-none focus:outline-none focus:border-accent-blue placeholder:text-surface-muted"
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                e.preventDefault();
                handleCommit();
              }
            }}
          />
          <button
            onClick={handleCommit}
            disabled={!message.trim() || loading}
            className="mt-1.5 w-full bg-accent-blue text-white rounded py-1 text-[11px] font-medium disabled:opacity-40 disabled:cursor-not-allowed hover:opacity-90 transition-opacity"
          >
            {loading ? "Committing..." : "Commit (Ctrl+Enter)"}
          </button>
        </div>
      )}

      {menu && (
        <div
          className="fixed z-50 min-w-36 rounded border border-surface-border bg-surface-panel py-1 text-xs shadow-xl"
          style={{ left: menu.x, top: menu.y }}
          onClick={(event) => event.stopPropagation()}
        >
          {!menu.entry.staged && (
            <button
              onClick={() => handleStage([menu.entry])}
              className="block w-full px-3 py-1.5 text-left text-surface-text hover:bg-surface-border/30"
            >
              Stage
            </button>
          )}
          {menu.entry.staged && (
            <button
              onClick={() => handleUnstage([menu.entry])}
              className="block w-full px-3 py-1.5 text-left text-surface-text hover:bg-surface-border/30"
            >
              Unstage
            </button>
          )}
          <button
            onClick={() => handleDiscard([menu.entry])}
            className="block w-full px-3 py-1.5 text-left text-diff-remove hover:bg-diff-remove/10"
          >
            Discard Changes
          </button>
        </div>
      )}
    </div>
  );
}
