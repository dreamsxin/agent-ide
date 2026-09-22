import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FileText } from "lucide-react";
import { useAgentStore } from "../../stores/useAgentStore";
import { isTauriRuntime } from "../../utils/tauri";
import { normalizeProjectMemoryInfo, type ProjectMemoryInfo } from "../../types/agent";

/**
 * 项目记忆（`AGENTS.md`）的状态，和唯一能改变它的那个动作。
 *
 * 为什么它需要一块界面：这份文件被注入**每一次** Agent 运行，而它的两种失效都没有任何症状 ——
 * 项目里根本没有这份文件（Agent 按通用习惯干活，用户以为它懂这个项目的规矩），或者它超过了
 * 注入上限（尾部被静默丢掉，而尾部恰好是最后写的那几条规则）。屏幕上不说，就没有别处会说。
 *
 * 起草不是一条新路径：按钮把后端给的提示词当普通提问发出去，Agent 用平常的写文件工具落地，
 * 改动照样进审查区、照样能撤销。所以这里没有任何新权限。
 */
export default function ProjectMemoryCard() {
  const sendPrompt = useAgentStore((s) => s.sendPrompt);
  const isStreaming = useAgentStore((s) => s.isStreaming);
  const [info, setInfo] = useState<ProjectMemoryInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sent, setSent] = useState(false);

  const load = useCallback(async () => {
    if (!isTauriRuntime()) return;
    setError(null);
    try {
      const parsed = normalizeProjectMemoryInfo(await invoke("get_project_memory"));
      if (!parsed) {
        // 读不懂就说读不懂，而不是渲染成"没有项目记忆" —— 后者会让用户去建一个已经存在的文件
        setError("The project memory status could not be read.");
        return;
      }
      setInfo(parsed);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);
  // 起草跑完之后再读一次：那次运行可能刚好把文件写出来了。同时清掉"已发出"那句话 ——
  // 它描述的是刚结束的那一次，留着会在后面无关的运行里继续挂着
  useEffect(() => {
    if (!isStreaming) {
      setSent(false);
      void load();
    }
  }, [isStreaming, load]);

  if (!isTauriRuntime()) {
    return null;
  }

  const actionable = info?.workspaceOpen === true;

  return (
    <div className="rounded border border-surface-border bg-surface-panel p-3">
      <div className="flex items-center gap-1.5 text-[11px] font-medium text-surface-text">
        <FileText aria-hidden="true" className="h-3.5 w-3.5" />
        Project memory (AGENTS.md)
      </div>
      <p className="mt-1 text-[10px] leading-relaxed text-surface-muted">
        {describe(info, error)}
      </p>
      {actionable && info && (
        <button
          type="button"
          disabled={isStreaming}
          onClick={() => {
            // 先发再说"已发出"：`sendPrompt` 自己会把失败写进 store 的 error，而这里
            // 提前显示成功会在"没配模型"时说一句彻头彻尾的假话
            void sendPrompt({ prompt: info.draftPrompt }).then(() => setSent(true));
          }}
          title="Ask the Agent to inspect this repo and propose the file as a reviewable change"
          className="mt-2 rounded border border-surface-border px-2 py-1 text-[10px] text-surface-text hover:bg-surface-border/30 disabled:opacity-40"
        >
          {info.exists ? "Update it with the Agent" : "Draft it with the Agent"}
        </button>
      )}
      {sent && (
        <p className="mt-1.5 text-[10px] text-surface-muted">
          Sent — the draft shows up in Chat, and the file itself arrives in the review area as a
          normal change you can reject.
        </p>
      )}
    </div>
  );
}

/**
 * 一句话说清此刻是哪一种状态。
 *
 * 四种状态必须区分开：没打开工作区、读不出来、没有这份文件、有但尾部被丢掉。前三种在
 * "规则没生效"上结果一样，而用户要做的事完全不同。
 *
 * "装得下"也不敢说成"全都发出去了"：上下文预算会按配额再削一次（项目记忆那一节占 15%），
 * 聊天里还能把这一节整个关掉 —— 说成"全部发送"就又是一句用户没法验证的假话。
 */
function describe(info: ProjectMemoryInfo | null, error: string | null): string {
  if (error) return error;
  if (!info) return "Reading the project memory status…";
  if (!info.workspaceOpen) {
    return "No workspace is open, so there is no project memory to read yet. Open a workspace folder first.";
  }
  if (!info.exists) {
    return `No AGENTS.md in this workspace, so every run starts without your project's conventions — the Agent guesses the build commands and the layout. It would go to ${info.path}.`;
  }
  if (info.truncated) {
    return `AGENTS.md is ${info.bytes} bytes, but only the first ${info.limit} reach a run — everything after that is dropped, and the tail is where the last rules you wrote are. Shorten it, or have the Agent tighten it.`;
  }
  return `AGENTS.md is ${info.bytes} of ${info.limit} bytes, so it fits the injection bound. A tight context budget can still trim it, and Chat can switch project memory off for one run.`;
}
