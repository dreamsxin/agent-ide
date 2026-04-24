import { useState, useEffect, useCallback } from "react";
import { Tree, type NodeRendererProps } from "react-arborist";
import type { NodeApi } from "react-arborist";
import { invoke } from "@tauri-apps/api/core";
import { useEditorStore } from "../../stores/useEditorStore";

// ====== Tauri 后端返回的文件条目 ======
interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
}

// ====== 树节点数据类型 ======
interface TreeNodeData {
  id: string;
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  /** 是否已加载子节点（用于懒加载） */
  childrenLoaded?: boolean;
  children?: TreeNodeData[];
}

// ====== 文件图标映射 ======
const FILE_ICONS: Record<string, string> = {
  ts: "🟦",
  tsx: "⚛️",
  js: "🟨",
  jsx: "⚛️",
  json: "📋",
  css: "🎨",
  html: "🌐",
  md: "📝",
  rs: "🦀",
  go: "🔵",
  py: "🐍",
  yaml: "⚙️",
  yml: "⚙️",
  toml: "⚙️",
  lock: "🔒",
  gitignore: "🙈",
};

function getFileIcon(name: string, isDir: boolean): string {
  if (isDir) return "📁";
  const ext = name.split(".").pop()?.toLowerCase() || "";
  if (FILE_ICONS[ext]) return FILE_ICONS[ext];
  // 特殊文件名
  const lower = name.toLowerCase();
  if (lower === "dockerfile") return "🐳";
  if (lower === "readme.md") return "📖";
  if (lower === "license") return "📜";
  return "📄";
}

// ====== 默认排除的目录 ======
const EXCLUDE_DIRS = new Set([
  "node_modules",
  ".git",
  "target",
  "dist",
  "build",
  ".workbuddy",
  "__pycache__",
  ".next",
]);

// ====== 语言检测 ======
function detectLanguage(path: string): string {
  const ext = path.split(".").pop() || "txt";
  const map: Record<string, string> = {
    ts: "typescript",
    tsx: "typescript",
    js: "javascript",
    jsx: "javascript",
    json: "json",
    css: "css",
    html: "html",
    md: "markdown",
    rs: "rust",
    go: "go",
    py: "python",
    yaml: "yaml",
    yml: "yaml",
    toml: "toml",
  };
  return map[ext] || "plaintext";
}

// ====== Tauri FS 条目 → TreeNodeData ======
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

// ====== 节点渲染组件 ======
function TreeNode({ node, style }: NodeRendererProps<TreeNodeData>) {
  const { data } = node;
  const icon = getFileIcon(data.name, data.isDir);

  return (
    <div
      style={style}
      className="flex items-center gap-1 py-0.5 px-1 hover:bg-surface-border/30 cursor-pointer text-xs text-surface-text"
      onClick={() => {
        if (!data.isDir) {
          const lang = detectLanguage(data.path);
          useEditorStore.getState().openFile({
            path: data.path,
            name: data.name,
            isDirty: false,
            language: lang,
          });
        } else {
          node.toggle();
        }
      }}
    >
      {/* 展开/折叠箭头 */}
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
    </div>
  );
}

// ====== 主组件 ======
export default function Explorer() {
  const [rootData, setRootData] = useState<TreeNodeData[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // 加载根目录
  const loadRoot = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      // 使用项目根目录
      const entries: FileEntry[] = await invoke("list_directory", {
        path: ".",
      });
      const nodes = entries
        .filter((e) => !EXCLUDE_DIRS.has(e.name))
        .map(fileEntryToNode);
      setRootData(nodes);
    } catch (e) {
      setError(`Failed to load directory: ${e}`);
      console.error(e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadRoot();
  }, [loadRoot]);

  // 懒加载子目录
  const handleToggle = useCallback(
    async (id: string) => {
      // 在数据树中查找节点
      const findAndUpdate = (
        nodes: TreeNodeData[]
      ): [TreeNodeData[], boolean] => {
        let changed = false;
        const updated = nodes.map((node) => {
          if (node.id === id && node.isDir && !node.childrenLoaded) {
            changed = true;
            // 异步加载，返回带占位的节点
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
      if (!changed) return; // 已加载或不是目录

      // 异步加载子节点
      try {
        const entries: FileEntry[] = await invoke("list_directory", {
          path: id,
        });
        const children = entries
          .filter((e) => !EXCLUDE_DIRS.has(e.name))
          .map(fileEntryToNode);

        // 更新树数据
        setRootData((prev) => {
          const updateNode = (nodes: TreeNodeData[]): TreeNodeData[] =>
            nodes.map((node) => {
              if (node.id === id) {
                return {
                  ...node,
                  children,
                  childrenLoaded: true,
                };
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

  // 节点激活（双击/回车）
  const handleActivate = useCallback((node: NodeApi<TreeNodeData>) => {
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
  }, []);

  return (
    <div className="h-full flex flex-col">
      <div className="px-2 py-1.5 text-[11px] font-semibold text-surface-muted uppercase tracking-wider border-b border-surface-border/50 no-select">
        Explorer
      </div>
      <div className="flex-1 overflow-hidden">
        {loading && (
          <div className="p-2 text-xs text-surface-muted">
            Loading files...
          </div>
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
            onActivate={handleActivate}
          >
            {TreeNode}
          </Tree>
        )}
        {!loading && !error && rootData.length === 0 && (
          <div className="p-2 text-xs text-surface-muted">
            No files found.
          </div>
        )}
      </div>
    </div>
  );
}
