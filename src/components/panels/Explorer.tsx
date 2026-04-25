import { useState, useEffect, useCallback, useRef } from "react";
import { Tree, type NodeRendererProps } from "react-arborist";
import type { NodeApi } from "react-arborist";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEditorStore } from "../../stores/useEditorStore";

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
  const [toast, setToast] = useState<string | null>(null);

  const explorerKey = useEditorStore((s) => s.explorerKey);
  const deletePath = useEditorStore((s) => s.deletePath);
  const createFile = useEditorStore((s) => s.createFile);
  const createDirectory = useEditorStore((s) => s.createDirectory);
  const renamePath = useEditorStore((s) => s.renamePath);
  const startWatching = useEditorStore((s) => s.startWatching);
  const reloadFile = useEditorStore((s) => s.reloadFile);

  const toastTimer = useRef<ReturnType<typeof setTimeout>>();

  // 加载根目录
  const loadRoot = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const entries: FileEntry[] = await invoke("list_directory", { path: "." });
      const nodes = entries
        .filter((e) => !EXCLUDE_DIRS.has(e.name))
        .map(fileEntryToNode);
      setRootData(nodes);
    } catch (e) {
      setError(`Failed to load directory: ${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

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

  // 右键菜单
  const handleContextMenu = useCallback(
    (e: React.MouseEvent, node: TreeNodeData) => {
      setContextMenu({
        x: e.clientX,
        y: e.clientY,
        node,
      });
    },
    []
  );

  const closeContextMenu = useCallback(() => {
    setContextMenu(null);
  }, []);

  // 全局点击关闭菜单
  useEffect(() => {
    if (!contextMenu) return;
    const handler = () => closeContextMenu();
    window.addEventListener("click", handler);
    window.addEventListener("contextmenu", handler);
    return () => {
      window.removeEventListener("click", handler);
      window.removeEventListener("contextmenu", handler);
    };
  }, [contextMenu, closeContextMenu]);

  // 新建文件
  const handleNewFile = useCallback(
    async (parentPath: string) => {
      const name = prompt("File name:");
      if (!name) return;
      const path = parentPath + "/" + name;
      try {
        await createFile(path);
      } catch (e) {
        alert(`Failed to create file: ${e}`);
      }
      closeContextMenu();
    },
    [createFile, closeContextMenu]
  );

  // 新建文件夹
  const handleNewFolder = useCallback(
    async (parentPath: string) => {
      const name = prompt("Folder name:");
      if (!name) return;
      const path = parentPath + "/" + name;
      try {
        await createDirectory(path);
      } catch (e) {
        alert(`Failed to create folder: ${e}`);
      }
      closeContextMenu();
    },
    [createDirectory, closeContextMenu]
  );

  // 重命名
  const handleRename = useCallback(
    async (node: TreeNodeData) => {
      const newName = prompt("New name:", node.name);
      if (!newName || newName === node.name) return;

      const parts = node.path.split(/[/\\]/);
      parts[parts.length - 1] = newName;
      const newPath = parts.join("/");

      try {
        await renamePath(node.path, newPath);
      } catch (e) {
        alert(`Failed to rename: ${e}`);
      }
      closeContextMenu();
    },
    [renamePath, closeContextMenu]
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
              const cwd = ".";
              handleNewFile(cwd);
            }}
            className="text-surface-muted hover:text-surface-text p-1 rounded hover:bg-surface-border/30 text-xs"
            title="New File"
          >
            📄+
          </button>
          <button
            onClick={() => {
              const cwd = ".";
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
      <div className="flex-1 overflow-hidden">
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
            onToggle={handleToggle}
            onActivate={(node) => {
              const { data } = node;
              if (!data.isDir) {
                const lang = detectLanguage(data.path);
                useEditorStore.getState().openFile({
                  path: data.path,
                  name: data.name,
                  isDirty: false,
                  language: lang,
                });
              }
            }}
            // @ts-expect-error react-arborist typing
            onContextMenu={(e: React.MouseEvent, node: NodeApi<TreeNodeData>) => {
              e.preventDefault();
              setContextMenu({ x: e.clientX, y: e.clientY, node: node.data });
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

      {/* 右键菜单 */}
      {contextMenu && (
        <div
          className="fixed z-50 bg-surface-panel border border-surface-border rounded-lg shadow-lg py-1 min-w-[160px]"
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

      {/* Toast 通知 */}
      {toast && (
        <div className="absolute bottom-2 left-1/2 -translate-x-1/2 bg-surface-panel border border-surface-border rounded-lg px-3 py-1.5 text-xs text-surface-text shadow-lg animate-fade-in z-50">
          {toast}
        </div>
      )}
    </div>
  );
}
