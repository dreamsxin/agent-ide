import { useState, useEffect, useCallback, useRef } from "react";
import { Tree, type NodeRendererProps } from "react-arborist";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEditorStore } from "../../stores/useEditorStore";
import { isTauriRuntime } from "../../utils/tauri";

// ====== 类型 ======
interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
}

interface TreeNodeData {
  id: string;
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  childrenLoaded?: boolean;
  children?: TreeNodeData[];
}

interface ContextMenuState {
  x: number;
  y: number;
  node: TreeNodeData;
}

interface FileClipboardState {
  node: TreeNodeData;
  operation: "copy";
}

interface NameDialogState {
  title: string;
  label: string;
  initialValue: string;
  confirmText: string;
  onConfirm: (value: string) => Promise<void>;
}

// ====== 文件图标 ======
const FILE_ICONS: Record<string, string> = {
  ts: "🟦", tsx: "⚛️", js: "🟨", jsx: "⚛️",
  json: "📋", css: "🎨", html: "🌐", md: "📝",
  rs: "🦀", go: "🔵", py: "🐍",
  yaml: "⚙️", yml: "⚙️", toml: "⚙️",
  lock: "🔒", gitignore: "🙈",
};

function getFileIcon(name: string, isDir: boolean): string {
  if (isDir) return "📁";
  const ext = name.split(".").pop()?.toLowerCase() || "";
  if (FILE_ICONS[ext]) return FILE_ICONS[ext];
  const lower = name.toLowerCase();
  if (lower === "dockerfile") return "🐳";
  if (lower === "readme.md") return "📖";
  return "📄";
}

const EXCLUDE_DIRS = new Set([
  "node_modules", ".git", "target", "dist", "build",
  ".workbuddy", "__pycache__", ".next",
]);

const CONTEXT_MENU_WIDTH = 210;
const CONTEXT_MENU_MAX_HEIGHT = 260;

function detectLanguage(path: string): string {
  const ext = path.split(".").pop() || "txt";
  const map: Record<string, string> = {
    ts: "typescript", tsx: "typescript", js: "javascript", jsx: "javascript",
    json: "json", css: "css", html: "html", md: "markdown",
    rs: "rust", go: "go", py: "python", yaml: "yaml", yml: "yaml", toml: "toml",
  };
  return map[ext] || "plaintext";
}

function fileEntryToNode(entry: FileEntry): TreeNodeData {
  return {
    id: entry.path,
    name: entry.name,
    path: entry.path,
    isDir: entry.is_dir,
    size: entry.size,
    childrenLoaded: false,
    children: entry.is_dir ? [] : undefined,
  };
}

function workspaceRootNode(workspacePath: string): TreeNodeData {
  return {
    id: workspacePath,
    name: basename(workspacePath) || "Workspace",
    path: workspacePath,
    isDir: true,
    size: 0,
    childrenLoaded: true,
    children: [],
  };
}

// ====== 树节点渲染 ======
function TreeNode({
  node,
  style,
  onContextMenu,
}: NodeRendererProps<TreeNodeData> & {
  onContextMenu: (e: React.MouseEvent, data: TreeNodeData) => void;
}) {
  const { data } = node;
  const icon = getFileIcon(data.name, data.isDir);
  const renamePath = useEditorStore((s) => s.activeFile);

  return (
    <div
      style={style}
      className="flex items-center gap-1 py-0.5 px-1 hover:bg-surface-border/30 cursor-pointer text-xs text-surface-text group"
      onClick={(e) => {
        if (!data.isDir) {
          const lang = detectLanguage(data.path);
          useEditorStore.getState().openFile({
            path: data.path,
            name: data.name,
            isDirty: false,
            language: lang,
          });
        } else {
          e.stopPropagation();
          node.toggle();
        }
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onContextMenu(e, data);
      }}
    >
      {data.isDir && (
        <span className="text-surface-muted w-3 text-center text-[10px] flex-shrink-0">
          {node.isOpen ? "▾" : "▸"}
        </span>
      )}
      {!data.isDir && <span className="w-3 flex-shrink-0" />}
      <span className="flex-shrink-0">{icon}</span>
      <span className={`truncate ${data.isDir ? "font-medium" : ""}`}>
        {data.name}
      </span>
      {/* 脏标记 */}
      {data.path === renamePath &&
        useEditorStore.getState().openFiles.find((f) => f.path === data.path)
          ?.isDirty && (
          <span className="ml-auto text-[10px] text-diff-modify">●</span>
        )}
    </div>
  );
}

// ====== 主组件 ======
export default function Explorer() {
  const [rootData, setRootData] = useState<TreeNodeData[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [fileClipboard, setFileClipboard] = useState<FileClipboardState | null>(null);
  const [nameDialog, setNameDialog] = useState<NameDialogState | null>(null);
  const [nameDialogValue, setNameDialogValue] = useState("");
  const [nameDialogError, setNameDialogError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const contextMenuRef = useRef<HTMLDivElement>(null);

  const explorerKey = useEditorStore((s) => s.explorerKey);
  const workspacePath = useEditorStore((s) => s.workspacePath);
  const deletePath = useEditorStore((s) => s.deletePath);
  const createFile = useEditorStore((s) => s.createFile);
  const createDirectory = useEditorStore((s) => s.createDirectory);
  const renamePath = useEditorStore((s) => s.renamePath);
  const copyPath = useEditorStore((s) => s.copyPath);
  const startWatching = useEditorStore((s) => s.startWatching);
  const reloadFile = useEditorStore((s) => s.reloadFile);

  const toastTimer = useRef<ReturnType<typeof setTimeout>>();

  // 加载根目录
  const loadRoot = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      if (!workspacePath) {
        setRootData([]);
        return;
      }
      if (!isTauriRuntime()) {
        setRootData([]);
        setError("File explorer is available in the Tauri app runtime.");
        return;
      }
      const entries: FileEntry[] = await invoke("list_directory", { path: workspacePath });
      const nodes = entries
        .filter((e) => !EXCLUDE_DIRS.has(e.name))
        .map(fileEntryToNode);
      setRootData(nodes);
    } catch (e) {
      setError(`Failed to load directory: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [workspacePath]);

  useEffect(() => {
    loadRoot();
  }, [loadRoot]);

  // 刷新（文件变更时）
  useEffect(() => {
    if (explorerKey > 0) {
      loadRoot();
    }
  }, [explorerKey, loadRoot]);

  // 启动文件监听
  useEffect(() => {
    if (!isTauriRuntime()) return;
    startWatching().catch(console.warn);

    // 监听后端文件变更事件
    let unlisten: (() => void) | undefined;
    listen<{ kind: string; paths: string[] }>("file-changed", (e) => {
      const { paths } = e.payload;
      // 检查是否有打开的文件被外部修改
      const openFiles = useEditorStore.getState().openFiles;
      for (const path of paths) {
        const openFile = openFiles.find((f) => f.path === path);
        if (openFile && !openFile.isDirty) {
          reloadFile(path);
        }
      }
      // 显示 toast
      showToast("Files changed externally. Refreshing...");
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(console.warn);

    return () => {
      unlisten?.();
    };
  }, []);

  function showToast(msg: string) {
    setToast(msg);
    if (toastTimer.current) clearTimeout(toastTimer.current);
    toastTimer.current = setTimeout(() => setToast(null), 3000);
  }

  // 懒加载子目录
  const handleToggle = useCallback(
    async (id: string) => {
      const findAndUpdate = (
        nodes: TreeNodeData[]
      ): [TreeNodeData[], boolean] => {
        let changed = false;
        const updated = nodes.map((node) => {
          if (node.id === id && node.isDir && !node.childrenLoaded) {
            changed = true;
            return node;
          }
          if (node.children) {
            const [newChildren, childChanged] = findAndUpdate(node.children);
            if (childChanged) {
              changed = true;
              return { ...node, children: newChildren };
            }
          }
          return node;
        });
        return [updated, changed];
      };

      const [, changed] = findAndUpdate(rootData);
      if (!changed) return;

      try {
        if (!isTauriRuntime()) return;
        const entries: FileEntry[] = await invoke("list_directory", { path: id });
        const children = entries
          .filter((e) => !EXCLUDE_DIRS.has(e.name))
          .map(fileEntryToNode);

        setRootData((prev) => {
          const updateNode = (nodes: TreeNodeData[]): TreeNodeData[] =>
            nodes.map((node) => {
              if (node.id === id) {
                return { ...node, children, childrenLoaded: true };
              }
              if (node.children) {
                return { ...node, children: updateNode(node.children) };
              }
              return node;
            });
          return updateNode(prev);
        });
      } catch (e) {
        console.error(`Failed to load children for ${id}:`, e);
      }
    },
    [rootData]
  );

  const closeContextMenu = useCallback(() => {
    setContextMenu(null);
  }, []);

  const openNameDialog = useCallback((state: NameDialogState) => {
    setNameDialog(state);
    setNameDialogValue(state.initialValue);
    setNameDialogError(null);
    closeContextMenu();
  }, [closeContextMenu]);

  const closeNameDialog = useCallback(() => {
    setNameDialog(null);
    setNameDialogValue("");
    setNameDialogError(null);
  }, []);

  // 右键菜单
  const handleContextMenu = useCallback(
    (e: React.MouseEvent, node?: TreeNodeData | null) => {
      if (!node) {
        if (!workspacePath) {
          closeContextMenu();
          return;
        }
        node = workspaceRootNode(workspacePath);
        e.preventDefault();
      }
      if (!node) {
        return;
      }
      const margin = 8;
      const x = Math.min(
        e.clientX,
        Math.max(margin, window.innerWidth - CONTEXT_MENU_WIDTH - margin)
      );
      const y = Math.min(
        e.clientY,
        Math.max(margin, window.innerHeight - CONTEXT_MENU_MAX_HEIGHT - margin)
      );
      setContextMenu({
        x,
        y,
        node,
      });
    },
    [closeContextMenu]
  );

  // 全局交互关闭菜单
  useEffect(() => {
    if (!contextMenu) return;

    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (target && contextMenuRef.current?.contains(target)) return;
      closeContextMenu();
    };
    const handleContextMenu = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (target && contextMenuRef.current?.contains(target)) return;
      closeContextMenu();
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeContextMenu();
      }
    };
    const handleScroll = () => closeContextMenu();

    document.addEventListener("pointerdown", handlePointerDown, true);
    document.addEventListener("contextmenu", handleContextMenu, true);
    document.addEventListener("keydown", handleKeyDown, true);
    window.addEventListener("scroll", handleScroll, true);
    window.addEventListener("blur", closeContextMenu);

    return () => {
      document.removeEventListener("pointerdown", handlePointerDown, true);
      document.removeEventListener("contextmenu", handleContextMenu, true);
      document.removeEventListener("keydown", handleKeyDown, true);
      window.removeEventListener("scroll", handleScroll, true);
      window.removeEventListener("blur", closeContextMenu);
    };
  }, [contextMenu, closeContextMenu]);

  // 新建文件
  const handleNewFile = useCallback(
    async (parentPath: string) => {
      openNameDialog({
        title: "New File",
        label: "File name",
        initialValue: "",
        confirmText: "Create",
        onConfirm: async (name) => {
          const path = joinPath(parentPath, name);
          await createFile(path);
          showToast(`Created file: ${name}`);
        },
      });
    },
    [createFile, openNameDialog]
  );

  // 新建文件夹
  const handleNewFolder = useCallback(
    async (parentPath: string) => {
      openNameDialog({
        title: "New Folder",
        label: "Folder name",
        initialValue: "",
        confirmText: "Create",
        onConfirm: async (name) => {
          const path = joinPath(parentPath, name);
          await createDirectory(path);
          showToast(`Created folder: ${name}`);
        },
      });
    },
    [createDirectory, openNameDialog]
  );

  // 重命名
  const handleRename = useCallback(
    async (node: TreeNodeData) => {
      openNameDialog({
        title: "Rename",
        label: "New name",
        initialValue: node.name,
        confirmText: "Rename",
        onConfirm: async (newName) => {
          if (newName === node.name) return;
          const parts = node.path.split(/[/\\]/);
          parts[parts.length - 1] = newName;
          const newPath = parts.join("/");
          await renamePath(node.path, newPath);
          showToast(`Renamed: ${newName}`);
        },
      });
    },
    [openNameDialog, renamePath]
  );

  // 复制文件/文件夹到内部剪贴板
  const handleCopy = useCallback(
    (node: TreeNodeData) => {
      setFileClipboard({ node, operation: "copy" });
      showToast(`Copied to Explorer clipboard: ${node.name}`);
      closeContextMenu();
    },
    [closeContextMenu, workspacePath]
  );

  const handlePaste = useCallback(
    async (targetNode?: TreeNodeData | null) => {
      if (!fileClipboard) return;
      const targetDirectory = targetNode?.isDir ? targetNode.path : workspacePath;
      if (!targetDirectory) return;
      try {
        const destination = await copyWithUniqueName(fileClipboard.node.path, targetDirectory, fileClipboard.node.name, copyPath);
        showToast(`Pasted: ${basename(destination)}`);
      } catch (e) {
        showToast(`Failed to paste: ${e}`);
      }
      closeContextMenu();
    },
    [closeContextMenu, copyPath, fileClipboard, workspacePath]
  );

  // 复制绝对路径
  const handleCopyFilePath = useCallback(
    async (node: TreeNodeData) => {
      try {
        await navigator.clipboard.writeText(node.path);
        showToast(`Copied file path: ${node.path}`);
      } catch (e) {
        alert(`Failed to copy file path: ${e}`);
      }
      closeContextMenu();
    },
    [closeContextMenu]
  );

  // 复制相对工作区路径
  const handleCopyRelativePath = useCallback(
    async (node: TreeNodeData) => {
      const relativePath = toWorkspaceRelativePath(workspacePath, node.path);
      try {
        await navigator.clipboard.writeText(relativePath);
        showToast(`Copied relative path: ${relativePath}`);
      } catch (e) {
        alert(`Failed to copy relative path: ${e}`);
      }
      closeContextMenu();
    },
    [closeContextMenu, workspacePath]
  );

  // 在系统文件管理器中显示
  const handleRevealInFileExplorer = useCallback(
    async (node: TreeNodeData) => {
      try {
        await invoke("reveal_in_file_explorer", { path: node.path });
        showToast(`Revealed: ${node.name}`);
      } catch (e) {
        alert(`Failed to reveal in file explorer: ${e}`);
      }
      closeContextMenu();
    },
    [closeContextMenu]
  );

  // 删除
  const handleDelete = useCallback(
    async (node: TreeNodeData) => {
      const type = node.isDir ? "folder" : "file";
      if (!confirm(`Delete ${type} "${node.name}"?`)) return;

      try {
        await deletePath(node.path);
      } catch (e) {
        alert(`Failed to delete: ${e}`);
      }
      closeContextMenu();
    },
    [deletePath, closeContextMenu]
  );

  return (
    <div className="h-full flex flex-col">
      {/* 标题栏 + 新建按钮 */}
      <div className="flex items-center justify-between px-2 py-1.5 border-b border-surface-border/50 no-select">
        <span className="text-[11px] font-semibold text-surface-muted uppercase tracking-wider">
          Explorer
        </span>
        <div className="flex gap-0.5">
          <button
            onClick={() => {
              const cwd = workspacePath;
              handleNewFile(cwd);
            }}
            className="text-surface-muted hover:text-surface-text p-1 rounded hover:bg-surface-border/30 text-xs"
            title="New File"
          >
            📄+
          </button>
          <button
            onClick={() => {
              const cwd = workspacePath;
              handleNewFolder(cwd);
            }}
            className="text-surface-muted hover:text-surface-text p-1 rounded hover:bg-surface-border/30 text-xs"
            title="New Folder"
          >
            📁+
          </button>
          <button
            onClick={loadRoot}
            className="text-surface-muted hover:text-surface-text p-1 rounded hover:bg-surface-border/30 text-xs"
            title="Refresh"
          >
            ↻
          </button>
        </div>
      </div>

      {/* 树 */}
      <div
        className="flex-1 overflow-hidden"
        onContextMenu={(event) => {
          event.preventDefault();
          handleContextMenu(event, null);
        }}
      >
        {loading && (
          <div className="p-2 text-xs text-surface-muted">Loading files...</div>
        )}
        {error && (
          <div className="p-2 text-xs text-diff-remove">
            {error}
            <button
              onClick={loadRoot}
              className="ml-2 underline hover:text-surface-text"
            >
              Retry
            </button>
          </div>
        )}
        {!loading && !error && rootData.length > 0 && (
          <Tree<TreeNodeData>
            data={rootData}
            idAccessor="id"
            childrenAccessor={(d) => d.children ?? null}
            height={(window.innerHeight || 600) - 120}
            width="100%"
            indent={14}
            rowHeight={26}
            overscanCount={20}
            openByDefault={false}
            onToggle={handleToggle}
            onActivate={(node) => {
              // 只处理目录切换（键盘 Enter），文件打开由 TreeNode onClick 处理
              if (node.data.isDir) {
                node.toggle();
              }
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              handleContextMenu(e, null);
            }}
          >
            {(props: NodeRendererProps<TreeNodeData>) => (
              <TreeNode {...props} onContextMenu={handleContextMenu} />
            )}
          </Tree>
        )}
        {!loading && !error && rootData.length === 0 && (
          <div className="p-2 text-xs text-surface-muted">No files found.</div>
        )}
      </div>

      {fileClipboard && (
        <div className="border-t border-surface-border/50 px-2 py-1.5 text-[10px] text-surface-muted">
          Copied: <span className="font-mono text-surface-text">{fileClipboard.node.name}</span>
          <button
            onClick={() => handlePaste(null)}
            className="ml-2 rounded border border-surface-border px-1.5 py-0.5 text-surface-text hover:bg-surface-border/30"
          >
            Paste
          </button>
        </div>
      )}

      {/* 右键菜单 */}
      {contextMenu && (
        <div
          ref={contextMenuRef}
          className="fixed z-50 max-h-[260px] min-w-[210px] overflow-auto rounded-lg border border-surface-border bg-surface-panel py-1 shadow-lg"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          {contextMenu.node.isDir && (
            <>
              <button
                onClick={() => handleNewFile(contextMenu.node.path)}
                className="w-full text-left px-3 py-1.5 text-xs text-surface-text hover:bg-surface-border/30 flex items-center gap-2"
              >
                <span>📄</span> New File
              </button>
              <button
                onClick={() => handleNewFolder(contextMenu.node.path)}
                className="w-full text-left px-3 py-1.5 text-xs text-surface-text hover:bg-surface-border/30 flex items-center gap-2"
              >
                <span>📁</span> New Folder
              </button>
              <div className="border-t border-surface-border my-0.5" />
            </>
          )}
          {contextMenu.node.isDir && fileClipboard && (
            <button
              onClick={() => handlePaste(contextMenu.node)}
              className="w-full text-left px-3 py-1.5 text-xs text-surface-text hover:bg-surface-border/30 flex items-center gap-2"
            >
              <span>📌</span> Paste
            </button>
          )}
          <button
            onClick={() => handleCopy(contextMenu.node)}
            className="w-full text-left px-3 py-1.5 text-xs text-surface-text hover:bg-surface-border/30 flex items-center gap-2"
          >
            <span>📋</span> Copy File
          </button>
          <button
            onClick={() => handleCopyFilePath(contextMenu.node)}
            className="w-full text-left px-3 py-1.5 text-xs text-surface-text hover:bg-surface-border/30 flex items-center gap-2"
          >
            <span>⧉</span> Copy File Path
          </button>
          <button
            onClick={() => handleCopyRelativePath(contextMenu.node)}
            className="w-full text-left px-3 py-1.5 text-xs text-surface-text hover:bg-surface-border/30 flex items-center gap-2"
          >
            <span>⧉</span> Copy Relative File Path
          </button>
          <button
            onClick={() => handleRevealInFileExplorer(contextMenu.node)}
            className="w-full text-left px-3 py-1.5 text-xs text-surface-text hover:bg-surface-border/30 flex items-center gap-2"
          >
            <span>📂</span> Reveal In File Explorer
          </button>
          <button
            onClick={() => handleRename(contextMenu.node)}
            className="w-full text-left px-3 py-1.5 text-xs text-surface-text hover:bg-surface-border/30 flex items-center gap-2"
          >
            <span>✏️</span> Rename
          </button>
          <button
            onClick={() => handleDelete(contextMenu.node)}
            className="w-full text-left px-3 py-1.5 text-xs text-diff-remove hover:bg-diff-remove/10 flex items-center gap-2"
          >
            <span>🗑️</span> Delete
          </button>
        </div>
      )}

      {nameDialog && (
        <div className="fixed inset-0 z-[60] flex items-start justify-center bg-black/20 pt-24">
          <form
            className="w-[360px] rounded border border-surface-border bg-surface-panel p-3 shadow-xl"
            onSubmit={async (event) => {
              event.preventDefault();
              const value = nameDialogValue.trim();
              if (!value) return;
              try {
                await nameDialog.onConfirm(value);
                closeNameDialog();
              } catch (e) {
                setNameDialogError(String(e));
              }
            }}
          >
            <div className="mb-3 text-sm font-semibold text-surface-text">{nameDialog.title}</div>
            <label className="block text-[11px] text-surface-muted">
              {nameDialog.label}
              <input
                autoFocus
                value={nameDialogValue}
                onChange={(event) => setNameDialogValue(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Escape") closeNameDialog();
                }}
                className="mt-1 w-full rounded border border-surface-border bg-surface-base px-2 py-1.5 font-mono text-xs text-surface-text outline-none focus:border-accent-blue"
              />
            </label>
            {nameDialogError && (
              <div className="mt-2 rounded border border-diff-remove/30 bg-diff-remove/10 px-2 py-1 text-[11px] text-diff-remove">
                {nameDialogError}
              </div>
            )}
            <div className="mt-3 flex justify-end gap-2">
              <button
                type="button"
                onClick={closeNameDialog}
                className="rounded border border-surface-border px-3 py-1 text-xs text-surface-muted hover:text-surface-text"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={!nameDialogValue.trim()}
                className="rounded bg-accent-blue px-3 py-1 text-xs font-medium text-white disabled:opacity-50"
              >
                {nameDialog.confirmText}
              </button>
            </div>
          </form>
        </div>
      )}

      {/* Toast 通知 */}
      {toast && (
        <div className="absolute bottom-2 left-1/2 -translate-x-1/2 bg-surface-panel border border-surface-border rounded-lg px-3 py-1.5 text-xs text-surface-text shadow-lg animate-fade-in z-50">
          {toast}
        </div>
      )}
    </div>
  );
}

function toWorkspaceRelativePath(workspacePath: string, targetPath: string) {
  const normalizedWorkspace = normalizePath(workspacePath).replace(/\/+$/, "");
  const normalizedTarget = normalizePath(targetPath);
  if (normalizedWorkspace && normalizedTarget.toLowerCase().startsWith(`${normalizedWorkspace.toLowerCase()}/`)) {
    return normalizedTarget.slice(normalizedWorkspace.length + 1);
  }
  return normalizedTarget;
}

function normalizePath(path: string) {
  return path.replace(/\\/g, "/");
}

function joinPath(parent: string, name: string) {
  const cleanParent = parent.replace(/[\\/]+$/, "");
  return `${cleanParent}/${name}`;
}

function basename(path: string) {
  return normalizePath(path).split("/").pop() || path;
}

async function copyWithUniqueName(
  sourcePath: string,
  parent: string,
  sourceName: string,
  copyPath: (src: string, dest: string) => Promise<void>
) {
  const candidates = copyNameCandidates(parent, sourceName);
  let lastError: unknown = null;
  for (const destination of candidates) {
    try {
      await copyPath(sourcePath, destination);
      return destination;
    } catch (error) {
      lastError = error;
      if (!String(error).toLowerCase().includes("destination already exists")) {
        throw error;
      }
    }
  }
  throw lastError ?? new Error("No available copy name");
}

function copyNameCandidates(parent: string, sourceName: string) {
  const dotIndex = sourceName.lastIndexOf(".");
  const hasExtension = dotIndex > 0;
  const stem = hasExtension ? sourceName.slice(0, dotIndex) : sourceName;
  const ext = hasExtension ? sourceName.slice(dotIndex) : "";
  return Array.from({ length: 50 }, (_, index) => {
    const suffix = index === 0 ? " Copy" : ` Copy ${index + 1}`;
    return joinPath(parent, `${stem}${suffix}${ext}`);
  });
}
