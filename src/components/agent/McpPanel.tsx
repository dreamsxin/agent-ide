import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "../../utils/tauri";
import { Plug, RefreshCw, Trash2 } from "lucide-react";

type McpServerConfig = {
  name: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  cwd?: string | null;
  enabled: boolean;
};

type McpConfig = {
  version: number;
  servers: McpServerConfig[];
};

type McpServerStatus = {
  name: string;
  connected: boolean;
  toolCount: number;
  error?: string | null;
};

type McpToolDescriptor = {
  server: string;
  tool: string;
  qualifiedName: string;
  description: string;
};

type McpDiscoveryResult = {
  servers: McpServerStatus[];
  tools: McpToolDescriptor[];
};

const EMPTY_DRAFT = { name: "", command: "", args: "" };

/**
 * MCP server 管理面板：配置 stdio server、发现工具、查看连接状态。
 * 发现到的工具会以 `mcp__{server}__{tool}` 注入到下一次 Agent 运行的原生工具列表。
 */
export default function McpPanel() {
  const available = isTauriRuntime();
  const [servers, setServers] = useState<McpServerConfig[]>([]);
  const [tools, setTools] = useState<McpToolDescriptor[]>([]);
  const [statuses, setStatuses] = useState<McpServerStatus[]>([]);
  const [draft, setDraft] = useState(EMPTY_DRAFT);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!available) return;
    void (async () => {
      try {
        const config = await invoke<McpConfig>("get_mcp_config");
        setServers(config.servers ?? []);
        setTools(await invoke<McpToolDescriptor[]>("get_mcp_tools"));
      } catch (err) {
        setError(String(err));
      }
    })();
  }, [available]);

  const persist = useCallback(async (next: McpServerConfig[]) => {
    setError("");
    try {
      const saved = await invoke<McpConfig>("save_mcp_config", {
        config: { version: 1, servers: next },
      });
      setServers(saved.servers ?? []);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleAdd = useCallback(async () => {
    const name = draft.name.trim();
    const command = draft.command.trim();
    if (!name || !command) return;
    if (servers.some((server) => server.name === name)) {
      setError(`Server '${name}' already exists`);
      return;
    }
    const args = draft.args.trim() ? draft.args.trim().split(/\s+/) : [];
    await persist([...servers, { name, command, args, env: {}, cwd: null, enabled: true }]);
    setDraft(EMPTY_DRAFT);
  }, [draft, persist, servers]);

  const handleDiscover = useCallback(async () => {
    setBusy(true);
    setError("");
    try {
      const result = await invoke<McpDiscoveryResult>("discover_mcp_tools");
      setStatuses(result.servers ?? []);
      setTools(result.tools ?? []);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }, []);

  if (!available) {
    return (
      <div className="mt-4 border-t border-surface-border pt-3 text-[10px] text-surface-muted">
        MCP servers require the Tauri runtime. Run <code className="rounded bg-surface-border/50 px-1">npm run tauri -- dev</code>.
      </div>
    );
  }

  return (
    <div className="mt-4 border-t border-surface-border pt-3">
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-[11px] font-medium text-surface-text">
          <Plug size={12} />
          MCP Servers
        </div>
        <button
          type="button"
          onClick={handleDiscover}
          disabled={busy || servers.length === 0}
          className="flex items-center gap-1 rounded border border-accent-purple/50 px-2 py-1 text-[10px] text-accent-purple hover:bg-accent-purple/10 disabled:cursor-not-allowed disabled:opacity-40"
        >
          <RefreshCw size={10} className={busy ? "animate-spin" : ""} />
          {busy ? "Connecting..." : "Discover Tools"}
        </button>
      </div>

      <div className="space-y-1">
        {servers.length === 0 && (
          <div className="text-[10px] text-surface-muted">
            No MCP servers configured. Example: <code className="rounded bg-surface-border/50 px-1">npx</code> with args{" "}
            <code className="rounded bg-surface-border/50 px-1">-y @modelcontextprotocol/server-filesystem .</code>
          </div>
        )}
        {servers.map((server) => {
          const status = statuses.find((entry) => entry.name === server.name);
          return (
            <div
              key={server.name}
              className="rounded border border-surface-border/60 bg-surface-panel/60 p-1.5 text-[10px]"
            >
              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={server.enabled}
                  onChange={(event) =>
                    void persist(
                      servers.map((entry) =>
                        entry.name === server.name ? { ...entry, enabled: event.target.checked } : entry
                      )
                    )
                  }
                  className="accent-accent-purple"
                />
                <span className="font-medium text-surface-text">{server.name}</span>
                {status && (
                  <span className={status.connected ? "text-accent-green" : "text-diff-remove"}>
                    {status.connected ? `${status.toolCount} tool(s)` : status.error ?? "failed"}
                  </span>
                )}
                <button
                  type="button"
                  onClick={() => void persist(servers.filter((entry) => entry.name !== server.name))}
                  className="ml-auto text-diff-remove hover:opacity-80"
                  title={`Remove ${server.name}`}
                >
                  <Trash2 size={11} />
                </button>
              </div>
              <div className="mt-0.5 truncate text-surface-muted">
                {server.command} {server.args.join(" ")}
              </div>
            </div>
          );
        })}
      </div>

      <div className="mt-2 grid grid-cols-3 gap-1">
        <input
          value={draft.name}
          onChange={(event) => setDraft((prev) => ({ ...prev, name: event.target.value }))}
          placeholder="name"
          className="rounded border border-surface-border bg-surface-bg px-1.5 py-1 text-[10px] text-surface-text"
        />
        <input
          value={draft.command}
          onChange={(event) => setDraft((prev) => ({ ...prev, command: event.target.value }))}
          placeholder="command"
          className="rounded border border-surface-border bg-surface-bg px-1.5 py-1 text-[10px] text-surface-text"
        />
        <input
          value={draft.args}
          onChange={(event) => setDraft((prev) => ({ ...prev, args: event.target.value }))}
          placeholder="args"
          className="rounded border border-surface-border bg-surface-bg px-1.5 py-1 text-[10px] text-surface-text"
        />
      </div>
      <button
        type="button"
        onClick={handleAdd}
        disabled={!draft.name.trim() || !draft.command.trim()}
        className="mt-1 w-full rounded border border-surface-border py-1 text-[10px] text-surface-muted hover:text-surface-text disabled:cursor-not-allowed disabled:opacity-40"
      >
        Add Server
      </button>

      {tools.length > 0 && (
        <div className="mt-2 space-y-0.5 rounded border border-surface-border/60 bg-surface-panel/60 p-1.5">
          <div className="text-[10px] text-surface-muted">
            {tools.length} tool(s) exposed to the Agent as native tool calls
          </div>
          {tools.map((tool) => (
            <div key={tool.qualifiedName} className="truncate text-[10px] text-surface-text">
              <code className="rounded bg-surface-border/50 px-1">{tool.qualifiedName}</code>{" "}
              <span className="text-surface-muted">{tool.description}</span>
            </div>
          ))}
        </div>
      )}

      {error && (
        <div className="mt-2 rounded border border-diff-remove/30 bg-diff-remove/10 px-2 py-1 text-[10px] text-diff-remove">
          {error}
        </div>
      )}
    </div>
  );
}
