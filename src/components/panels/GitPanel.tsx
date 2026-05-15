import { useState, useEffect, useCallback } from "react";
import { useGitStore } from "../../stores/useGitStore";
import { useLogStore } from "../../stores/useLogStore";
import type { GitStatusEntry } from "../../types/project";

const STATUS_ICONS: Record<string, { icon: string; color: string }> = {
  modified: { icon: "M", color: "text-diff-modify" },
  added: { icon: "A", color: "text-accent-green" },
  deleted: { icon: "D", color: "text-diff-remove" },
  untracked: { icon: "U", color: "text-accent-green" },
  renamed: { icon: "R", color: "text-accent-blue" },
};

export default function GitPanel() {
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
  const addLog = useLogStore((s) => s.addLog);

  const [message, setMessage] = useState("");
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [projectPath, setProjectPath] = useState(".");
  const [menu, setMenu] = useState<{
    x: number;
    y: number;
    entry: GitStatusEntry;
  } | null>(null);

  // 自动检测项目路径
  useEffect(() => {
    const load = async () => {
      try {
        setProjectPath(".");
        fetchStatus(".");
      } catch {
        setProjectPath(".");
        fetchStatus(".");
      }
    };
    load();
  }, [fetchStatus]);

  const handleRefresh = useCallback(() => {
    fetchStatus(projectPath);
    addLog({
      time: new Date().toLocaleTimeString(),
      level: "info",
      source: "git",
      message: "Git status refreshed",
    });
  }, [fetchStatus, projectPath, addLog]);

  const handleFileClick = useCallback(
    (file: GitStatusEntry) => {
      if (selectedFile === file.path) {
        clearDiff();
        setSelectedFile(null);
        return;
      }
      setSelectedFile(file.path);
      fetchDiff(projectPath, file.path);
    },
    [selectedFile, clearDiff, fetchDiff, projectPath]
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
      fetchStatus(projectPath);
      clearDiff();
      setSelectedFile(null);
    }
  }, [message, commit, projectPath, addLog, fetchStatus, clearDiff]);

  const refreshAfterAction = useCallback(async (logMessage: string) => {
    addLog({
      time: new Date().toLocaleTimeString(),
      level: "success",
      source: "git",
      message: logMessage,
    });
    await fetchStatus(projectPath);
    clearDiff();
    setSelectedFile(null);
    setMenu(null);
  }, [addLog, fetchStatus, projectPath, clearDiff]);

  const handleStage = useCallback(async (entry: GitStatusEntry) => {
    if (await stageFiles(projectPath, [entry.path])) {
      await refreshAfterAction(`Staged ${entry.path}`);
    }
  }, [stageFiles, projectPath, refreshAfterAction]);

  const handleUnstage = useCallback(async (entry: GitStatusEntry) => {
    if (await unstageFiles(projectPath, [entry.path])) {
      await refreshAfterAction(`Unstaged ${entry.path}`);
    }
  }, [unstageFiles, projectPath, refreshAfterAction]);

  const handleDiscard = useCallback(async (entry: GitStatusEntry) => {
    const ok = window.confirm(`Discard changes in ${entry.path}? This cannot be undone.`);
    if (!ok) return;
    if (await discardFiles(projectPath, [entry.path])) {
      await refreshAfterAction(`Discarded ${entry.path}`);
    }
  }, [discardFiles, projectPath, refreshAfterAction]);

  const handleContextMenu = useCallback((event: React.MouseEvent, entry: GitStatusEntry) => {
    event.preventDefault();
    setMenu({ x: event.clientX, y: event.clientY, entry });
  }, []);

  // Group entries by status
  const staged = status?.entries.filter((e) => e.staged) ?? [];
  const unstaged = status?.entries.filter((e) => !e.staged) ?? [];

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("keydown", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", close);
    };
  }, [menu]);

  const renderEntry = (entry: GitStatusEntry) => {
    const info = STATUS_ICONS[entry.status] ?? { icon: "?", color: "text-surface-muted" };
    const isSelected = selectedFile === entry.path;
    return (
      <div
        key={`${entry.staged ? "staged" : "unstaged"}-${entry.path}-${entry.status}`}
        onClick={() => handleFileClick(entry)}
        onContextMenu={(event) => handleContextMenu(event, entry)}
        className={`group flex items-center gap-1.5 px-3 py-1 cursor-pointer transition-colors ${
          isSelected
            ? "bg-accent-blue/10 text-surface-text"
            : "text-surface-text hover:bg-surface-border/20"
        }`}
      >
        <span className={`w-4 text-center font-bold text-[10px] ${info.color}`}>
          {info.icon}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px]">{entry.path}</span>
        <button
          onClick={(event) => {
            event.stopPropagation();
            entry.staged ? handleUnstage(entry) : handleStage(entry);
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
      {/* Header */}
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

      {/* Error */}
      {error && (
        <div className="px-3 py-2 text-diff-remove text-[10px] border-b border-surface-border">
          {error}
        </div>
      )}

      {/* Branch info */}
      {status && (
        <div className="px-3 py-2 border-b border-surface-border">
          <div className="flex items-center gap-1.5 text-surface-text">
            <span className="text-accent-blue">⎇</span>
            <span className="font-mono">{status.branch}</span>
          </div>
          {status.ahead > 0 && (
            <div className="text-accent-blue text-[10px] mt-0.5">
              ↑{status.ahead} ahead
            </div>
          )}
          {status.behind > 0 && (
            <div className="text-diff-modify text-[10px] mt-0.5">
              ↓{status.behind} behind
            </div>
          )}
        </div>
      )}

      {/* Loading */}
      {loading && (
        <div className="px-3 py-2 text-surface-muted animate-pulse">Loading...</div>
      )}

      {/* Staged changes */}
      {staged.length > 0 && (
        <div className="overflow-auto">
          <div className="px-3 py-1.5 text-surface-muted font-semibold text-[10px] uppercase tracking-wider">
            Staged Changes ({staged.length})
          </div>
          {staged.map(renderEntry)}
        </div>
      )}

      {/* Unstaged */}
      {unstaged.length > 0 && (
        <div className="flex-1 overflow-auto">
          <div className="px-3 py-1.5 text-surface-muted font-semibold text-[10px] uppercase tracking-wider">
            Changes ({unstaged.length})
          </div>
          {unstaged.map(renderEntry)}
        </div>
      )}

      {/* Empty state */}
      {status && staged.length === 0 && unstaged.length === 0 && (
        <div className="px-3 py-4 text-surface-muted text-center">
          No changes detected.
        </div>
      )}

      {/* No repo */}
      {!status && !loading && !error && (
        <div className="px-3 py-4 text-surface-muted text-center">
          No git repository found.
        </div>
      )}

      {/* Diff viewer */}
      {diff && selectedFile && (
        <div className="border-t border-surface-border max-h-60 overflow-auto">
          <div className="px-3 py-1.5 text-surface-muted font-semibold text-[10px] uppercase sticky top-0 bg-surface-panel">
            Diff: {selectedFile}
          </div>
          <pre className="px-3 pb-2 font-mono text-[10px] text-surface-text whitespace-pre-wrap leading-relaxed">
            {diff.split("\n").map((line, i) => {
              let color = "text-surface-text";
              if (line.startsWith("+")) color = "text-diff-add";
              else if (line.startsWith("-")) color = "text-diff-remove";
              else if (line.startsWith("@@")) color = "text-accent-blue";
              else if (line.startsWith("diff") || line.startsWith("---") || line.startsWith("+++"))
                color = "text-surface-muted";

              return (
                <div key={i} className={color}>
                  {line}
                </div>
              );
            })}
          </pre>
        </div>
      )}

      {/* Commit input */}
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
              onClick={() => handleStage(menu.entry)}
              className="block w-full px-3 py-1.5 text-left text-surface-text hover:bg-surface-border/30"
            >
              Stage
            </button>
          )}
          {menu.entry.staged && (
            <button
              onClick={() => handleUnstage(menu.entry)}
              className="block w-full px-3 py-1.5 text-left text-surface-text hover:bg-surface-border/30"
            >
              Unstage
            </button>
          )}
          <button
            onClick={() => handleDiscard(menu.entry)}
            className="block w-full px-3 py-1.5 text-left text-diff-remove hover:bg-diff-remove/10"
          >
            Discard Changes
          </button>
        </div>
      )}
    </div>
  );
}
