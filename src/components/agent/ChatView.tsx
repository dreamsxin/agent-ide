import { useState, useRef, useEffect, useCallback } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { useProblemStore } from "../../stores/useProblemStore";
import { useTaskStore } from "../../stores/useTaskStore";
import { useLogStore } from "../../stores/useLogStore";
import { withIdeRuntimeContext, type IdeRuntimeContextOptions } from "../../utils/agentRuntimeContext";
import ReactMarkdown from "react-markdown";
import type { AgentState, ContextCompressionMode, ContextEstimateResponse } from "../../types/agent";
import type { ProblemEntry } from "../../stores/useProblemStore";
import type { ProjectTaskRunState } from "../../stores/useTaskStore";
import type { LogEntry } from "../../types/project";

type ChatContextOptions = {
  activeFile: boolean;
  selection: boolean;
  openFiles: boolean;
  problems: boolean;
  failedTask: boolean;
  terminalOutput: boolean;
  logs: boolean;
  gitDiff: boolean;
  projectTree: boolean;
};

const DEFAULT_CONTEXT_OPTIONS: ChatContextOptions = {
  activeFile: true,
  selection: true,
  openFiles: true,
  problems: true,
  failedTask: true,
  terminalOutput: true,
  logs: true,
  gitDiff: true,
  projectTree: true,
};

const CONTEXT_OPTIONS_KEY = "agent-ide-chat-context-options";

/** 自动闭合未关闭的代码块，防止整个后缀被渲染为代码 */
function sanitizeMarkdown(raw: string): string {
  const lines = raw.split('\n');
  let inBlock = false;
  const result: string[] = [];

  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.startsWith('```')) {
      if (!inBlock) {
        inBlock = true;
      } else {
        inBlock = false;
      }
    }
    result.push(line);
  }

  // 未闭合的代码块自动补上 ```
  if (inBlock) {
    result.push('```');
  }

  return result.join('\n');
}

/** 将 markdown 渲染为 HTML，支持流式不完整代码块 */
/** 各状态对应的 UI 信息 */
const STATE_INFO: Record<AgentState, { label: string; spinner: boolean }> = {
  idle:         { label: "Ready",         spinner: false },
  thinking:     { label: "Thinking…",     spinner: true },
  planning:     { label: "Planning…",     spinner: true },
  acting:       { label: "Executing…",    spinner: true },
  reviewing:    { label: "Reviewing…",    spinner: true },
  waiting_user: { label: "Awaiting input", spinner: false },
  done:         { label: "Done",           spinner: false },
  error:        { label: "Error",          spinner: false },
};

/** 单条消息组件 */
function MessageBubble({
  msg,
  isStreamingBubble,
}: {
  msg: { id: string; role: string; content: string };
  isStreamingBubble: boolean;
}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(msg.content);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // fallback
    }
  }, [msg.content]);

  const isUser = msg.role === "user";
  const isSystem = msg.role === "system";
  const isAgent = msg.role === "agent";

  return (
    <div
      className={`animate-fade-in ${
        isUser ? "flex justify-end" : "flex justify-start"
      }`}
    >
      <div
        className={`relative group max-w-[90%] rounded-lg px-3 py-2 text-xs leading-relaxed ${
          isUser
            ? "bg-accent-blue text-white"
            : isSystem
            ? "bg-surface-border/30 text-surface-muted text-center w-full"
            : "bg-surface-border/50 text-surface-text"
        }`}
      >
        {/* 内容 */}
        {isAgent ? (
          <div className="markdown-body">
            <ReactMarkdown skipHtml>{sanitizeMarkdown(msg.content)}</ReactMarkdown>
          </div>
        ) : (
          <div className="whitespace-pre-wrap">{msg.content}</div>
        )}

        {/* 流式光标 */}
        {isAgent && isStreamingBubble && (
          <span className="inline-block w-1.5 h-3 bg-accent-purple ml-0.5 animate-pulse align-middle" />
        )}

        {/* 复制按钮（agent 消息 hover 时显示） */}
        {isAgent && msg.content && (
          <button
            onClick={handleCopy}
            title="Copy raw content"
            className="absolute top-1 right-1 opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded hover:bg-surface-border/50 text-surface-muted hover:text-surface-text text-[10px]"
          >
            {copied ? "✓" : "📋"}
          </button>
        )}
      </div>
    </div>
  );
}

function ContextToggle({
  label,
  detail,
  checked,
  onChange,
}: {
  label: string;
  detail: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex min-w-0 items-center gap-2 rounded border border-surface-border/60 px-2 py-1 text-surface-text">
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
        className="h-3 w-3 flex-shrink-0 accent-accent-blue"
      />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <span className="max-w-[82px] truncate text-[10px] text-surface-muted">{detail}</span>
    </label>
  );
}

export default function ChatView() {
  const messages = useAgentStore((s) => s.messages);
  const addMessage = useAgentStore((s) => s.addMessage);
  const updateMessage = useAgentStore((s) => s.updateMessage);
  const activeSddArtifact = useAgentStore((s) => s.activeSddArtifact);
  const ghostSuggestions = useAgentStore((s) => s.ghostSuggestions);
  const setGhostSuggestions = useAgentStore((s) => s.setGhostSuggestions);
  const dismissGhostSuggestion = useAgentStore((s) => s.dismissGhostSuggestion);
  const updateActiveSddMarkdown = useAgentStore((s) => s.updateActiveSddMarkdown);
  const saveActiveSdd = useAgentStore((s) => s.saveActiveSdd);
  const promoteSddToCodePrompt = useAgentStore((s) => s.promoteSddToCodePrompt);
  const [input, setInput] = useState("");
  const [sddSaveMessage, setSddSaveMessage] = useState("");
  const [contextPreviewOpen, setContextPreviewOpen] = useState(false);
  const [contextOptions, setContextOptions] = useState<ChatContextOptions>(() => loadContextOptions());
  const [contextEstimate, setContextEstimate] = useState<ContextEstimateResponse | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  const agentState = useAgentStore((s) => s.state);
  const agentMode = useAgentStore((s) => s.mode);
  const ideMode = useAgentStore((s) => s.ideMode);
  const streamContent = useAgentStore((s) => s.streamContent);
  const isStreaming = useAgentStore((s) => s.isStreaming);
  const sendPrompt = useAgentStore((s) => s.sendPrompt);
  const estimateContext = useAgentStore((s) => s.estimateContext);
  const stopAgent = useAgentStore((s) => s.stopAgent);
  const llmProfiles = useAgentStore((s) => s.llmProfiles);
  const activeProfileId = useAgentStore((s) => s.activeProfileId);
  const chatProfileId = useAgentStore((s) => s.chatProfileId);
  const chatContextCompression = useAgentStore((s) => s.chatContextCompression);
  const contextCompression = useAgentStore((s) => s.contextCompression);
  const setChatProfileId = useAgentStore((s) => s.setChatProfileId);
  const setChatContextCompression = useAgentStore((s) => s.setChatContextCompression);

  const activeFile = useEditorStore((s) => s.activeFile);
  const openFiles = useEditorStore((s) => s.openFiles);
  const fileContents = useEditorStore((s) => s.fileContents);
  const selectedText = useEditorStore((s) => s.selectedText);
  const problems = useProblemStore((s) => s.problems);
  const taskRuns = useTaskStore((s) => s.taskRuns);
  const terminalOutput = useTaskStore((s) => s.terminalOutput);
  const logs = useLogStore((s) => s.logs);

  useEffect(() => {
    const suggestions = buildGhostSuggestions(problems, taskRuns, logs);
    setGhostSuggestions(suggestions);
  }, [logs, problems, setGhostSuggestions, taskRuns]);

  useEffect(() => {
    persistContextOptions(contextOptions);
  }, [contextOptions]);

  const isActing =
    agentState !== "idle" &&
    agentState !== "done" &&
    agentState !== "error" &&
    agentState !== "waiting_user";

  const info = STATE_INFO[agentState] ?? STATE_INFO.idle;
  const isSending = isActing;
  const selectedProfileId = chatProfileId ?? activeProfileId;
  const selectedContextMode = chatContextCompression ?? contextCompression;
  const selectedProfile = llmProfiles.find((profile) => profile.id === selectedProfileId);
  const failedTaskCount = Object.values(taskRuns).filter((task) => task.status === "failed").length;
  const terminalSessionCount = Object.values(terminalOutput).filter((output) => output.trim()).length;
  const warningLogCount = logs.filter((log) => log.level === "error" || log.level === "warn").length;
  const selectedContextItems = [
    contextOptions.activeFile && activeFile ? "active file" : null,
    contextOptions.selection && selectedText ? "selection" : null,
    contextOptions.openFiles && openFiles.length > 0 ? `${openFiles.length} open file${openFiles.length === 1 ? "" : "s"}` : null,
    contextOptions.problems && problems.length > 0 ? `${problems.length} problem${problems.length === 1 ? "" : "s"}` : null,
    contextOptions.failedTask && failedTaskCount > 0 ? `${failedTaskCount} failed run${failedTaskCount === 1 ? "" : "s"}` : null,
    contextOptions.terminalOutput && terminalSessionCount > 0 ? `${terminalSessionCount} terminal${terminalSessionCount === 1 ? "" : "s"}` : null,
    contextOptions.logs && warningLogCount > 0 ? `${warningLogCount} warning/error log${warningLogCount === 1 ? "" : "s"}` : null,
    contextOptions.gitDiff ? "git diff" : null,
    contextOptions.projectTree ? "project tree" : null,
  ].filter(Boolean);
  const estimatedSelectedTokens = contextEstimate?.estimatedTokens ?? 0;

  // 当前流式消息 ID：用于实时显示
  const streamingMsgId = useRef<string | null>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamContent]);

  // 流式内容变化时更新消息列表
  useEffect(() => {
    if (!isStreaming) {
      streamingMsgId.current = null;
      return;
    }
    const msgId = streamingMsgId.current;
    if (msgId) {
      updateMessage(msgId, { content: streamContent });
    } else if (streamContent) {
      const newId = `stream-${Date.now()}`;
      streamingMsgId.current = newId;
      addMessage({
        id: newId,
        role: "agent",
        content: streamContent,
        timestamp: Date.now(),
      });
    }
  }, [streamContent, isStreaming, addMessage, updateMessage]);

  // 流结束时固化消息
  useEffect(() => {
    if (!isStreaming && streamingMsgId.current) {
      streamingMsgId.current = null;
    }
  }, [isStreaming]);

  const buildContext = useCallback(() => {
    return {
      activeFile: contextOptions.activeFile ? activeFile ?? undefined : undefined,
      activeFileContent: contextOptions.activeFile && activeFile ? fileContents[activeFile] : undefined,
      selection: contextOptions.selection ? selectedText ?? undefined : undefined,
      contextFiles: contextOptions.openFiles ? openFiles.map((file) => file.path) : [],
    };
  }, [activeFile, fileContents, selectedText, openFiles, contextOptions]);

  useEffect(() => {
    let cancelled = false;
    const ctx = buildContext();
    const handle = window.setTimeout(() => {
      void estimateContext({
        ...ctx,
        profileId: selectedProfileId || undefined,
        contextCompression: selectedContextMode,
        contextSources: {
          includeGitDiff: contextOptions.gitDiff,
          includeProjectTree: contextOptions.projectTree,
        },
      }).then((estimate) => {
        if (!cancelled) setContextEstimate(estimate);
      });
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [buildContext, estimateContext, selectedProfileId, selectedContextMode, contextOptions.gitDiff, contextOptions.projectTree]);

  const handleSend = useCallback(async () => {
    if (!input.trim() || isActing) return;

    const content = input.trim();
    addMessage({
      id: Date.now().toString(),
      role: "user",
      content,
      timestamp: Date.now(),
    });
    setInput("");

    const ctx = buildContext();
    const runtimeContextOptions: IdeRuntimeContextOptions = {
      includeFailedTask: contextOptions.failedTask,
      includeProblems: contextOptions.problems,
      includeTerminalOutput: contextOptions.terminalOutput,
      includeLogs: contextOptions.logs,
    };
    await sendPrompt({
      prompt: withIdeRuntimeContext(content, runtimeContextOptions),
      profileId: selectedProfileId || undefined,
      contextCompression: selectedContextMode,
      contextSources: {
        includeGitDiff: contextOptions.gitDiff,
        includeProjectTree: contextOptions.projectTree,
      },
      ...ctx,
    });
  }, [input, isActing, sendPrompt, selectedProfileId, selectedContextMode, buildContext, addMessage, contextOptions]);

  const handleSaveSdd = useCallback(async () => {
    setSddSaveMessage("");
    try {
      const saved = await saveActiveSdd(false);
      if (saved) {
        setSddSaveMessage(`Saved ${saved.path}`);
      }
    } catch (err) {
      setSddSaveMessage(String(err));
    }
  }, [saveActiveSdd]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleStop = () => {
    stopAgent();
  };

  return (
    <div className="flex flex-col h-full">
      {/* 消息列表 */}
      <div className="flex-1 overflow-auto p-3 space-y-3">
        {messages.map((msg) => (
          <MessageBubble
            key={msg.id}
            msg={msg}
            isStreamingBubble={
              msg.role === "agent" && isStreaming && msg.id === streamingMsgId.current
            }
          />
        ))}
        {activeSddArtifact && (
          <div className="rounded border border-surface-border bg-surface-base/70 p-2 text-xs text-surface-text">
            <div className="mb-2 flex items-center justify-between gap-2">
              <div className="min-w-0">
                <div className="truncate font-semibold">{activeSddArtifact.title}</div>
                <div className="text-[10px] text-surface-muted">
                  docs/design/{activeSddArtifact.slug}.md · {activeSddArtifact.status}
                </div>
              </div>
              <div className="flex flex-shrink-0 gap-1">
                <button
                  type="button"
                  onClick={handleSaveSdd}
                  className="rounded border border-accent-blue/50 px-2 py-1 text-[11px] text-accent-blue hover:bg-accent-blue/10"
                >
                  Save
                </button>
                <button
                  type="button"
                  onClick={() => {
                    promoteSddToCodePrompt();
                    setInput(`Implement the approved SDD: ${activeSddArtifact.title}`);
                  }}
                  className="rounded bg-accent-blue px-2 py-1 text-[11px] text-white hover:bg-blue-700"
                >
                  Code
                </button>
              </div>
            </div>
            {activeSddArtifact.reviewFindings.length > 0 && (
              <div className="mb-2 rounded border border-diff-modify/30 bg-diff-modify/10 p-2 text-[11px]">
                <div className="mb-1 font-medium text-diff-modify">Review Findings</div>
                {activeSddArtifact.reviewFindings.map((finding, index) => (
                  <div key={`${finding}-${index}`} className="truncate text-surface-muted" title={finding}>
                    - {finding}
                  </div>
                ))}
              </div>
            )}
            <textarea
              value={activeSddArtifact.markdown}
              onChange={(event) => updateActiveSddMarkdown(event.target.value)}
              rows={12}
              className="h-56 w-full resize-y rounded border border-surface-border bg-surface-panel p-2 font-mono text-[11px] leading-relaxed text-surface-text outline-none focus:border-accent-blue"
            />
            {sddSaveMessage && (
              <div className="mt-1 truncate text-[10px] text-surface-muted" title={sddSaveMessage}>
                {sddSaveMessage}
              </div>
            )}
          </div>
        )}
        {ghostSuggestions.length > 0 && (
          <div className="space-y-1.5 rounded border border-surface-border bg-surface-base/50 p-2 text-xs">
            <div className="text-[10px] font-semibold uppercase tracking-wide text-surface-muted">
              Ghost Suggestions
            </div>
            {ghostSuggestions.map((suggestion) => (
              <div
                key={suggestion.id}
                className="grid grid-cols-[minmax(0,1fr)_auto] gap-2 rounded border border-surface-border/60 bg-surface-panel/70 p-2"
              >
                <div className="min-w-0">
                  <div className="truncate font-medium text-surface-text">{suggestion.title}</div>
                  <div className="truncate text-[10px] text-surface-muted" title={suggestion.detail}>
                    {suggestion.detail}
                  </div>
                </div>
                <div className="flex gap-1">
                  <button
                    type="button"
                    onClick={() => {
                      setInput(suggestion.prompt);
                      dismissGhostSuggestion(suggestion.id);
                    }}
                    className="rounded border border-accent-blue/50 px-2 py-1 text-[11px] text-accent-blue hover:bg-accent-blue/10"
                  >
                    Use
                  </button>
                  <button
                    type="button"
                    onClick={() => dismissGhostSuggestion(suggestion.id)}
                    className="rounded border border-surface-border px-2 py-1 text-[11px] text-surface-muted hover:text-surface-text"
                  >
                    Dismiss
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
        <div ref={bottomRef} />
      </div>

      {/* 输入区 */}
      <div className="p-2 border-t border-surface-border">
        {/* 状态指示条 */}
        <div className="flex items-center gap-2 mb-1.5 px-0.5">
          {info.spinner && (
            <span className="inline-block w-2.5 h-2.5 border-2 border-surface-muted border-t-accent-blue rounded-full animate-spin flex-shrink-0" />
          )}
          <span className={`text-[11px] font-medium ${info.spinner ? "text-accent-blue" : "text-surface-muted"}`}>
            {info.label}
          </span>
          {agentState !== "idle" && agentState !== "error" && (
            <span className="text-[10px] text-surface-muted ml-auto">
              {ideMode}/{agentMode}
            </span>
          )}
        </div>

        <div className="mb-1.5 grid grid-cols-[minmax(0,1fr)_112px] gap-1.5">
          <select
            value={selectedProfileId}
            onChange={(event) => setChatProfileId(event.target.value || null)}
            disabled={isSending || llmProfiles.length === 0}
            className="min-w-0 rounded border border-surface-border bg-surface-base px-2 py-1 text-[11px] text-surface-text outline-none focus:border-accent-blue disabled:cursor-not-allowed disabled:opacity-50"
            title="LLM profile for this chat run"
          >
            {llmProfiles.length === 0 ? (
              <option value="">No provider configured</option>
            ) : (
              llmProfiles.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.name} · {profile.model}
                </option>
              ))
            )}
          </select>
          <select
            value={selectedContextMode}
            onChange={(event) => setChatContextCompression(event.target.value as ContextCompressionMode)}
            disabled={isSending}
            className="rounded border border-surface-border bg-surface-base px-2 py-1 text-[11px] text-surface-text outline-none focus:border-accent-blue disabled:cursor-not-allowed disabled:opacity-50"
            title="Context compression mode for this chat run"
          >
            <option value="focused">Mode: Focused</option>
            <option value="compact">Mode: Compact</option>
            <option value="budgeted">Mode: Budgeted</option>
            <option value="full">Mode: Full</option>
          </select>
        </div>
        {(selectedProfile?.effectiveInputTokens !== undefined || contextEstimate) && (
          <div className="mb-1.5 px-0.5 text-[10px] text-surface-muted">
            Estimated input budget:{" "}
            <span className="font-mono text-surface-text">
              {(contextEstimate?.inputBudgetTokens ?? selectedProfile?.effectiveInputTokens ?? 0).toLocaleString()}
            </span>{" "}
            tokens · selected context{" "}
            <span className="font-mono text-surface-text">
              {estimatedSelectedTokens.toLocaleString()}
            </span>
            {contextEstimate?.trimmed ? (
              <span className="text-diff-remove"> · trimmed</span>
            ) : null}
          </div>
        )}
        <div className="mb-1.5 rounded border border-surface-border bg-surface-base/60">
          <button
            type="button"
            onClick={() => setContextPreviewOpen((open) => !open)}
            className="grid w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-2 px-2 py-1 text-left text-[11px] text-surface-muted hover:bg-surface-border/20"
            title="Preview and choose what the Agent receives as context"
          >
            <span className="truncate">
              Context: {selectedContextItems.length > 0 ? selectedContextItems.join(", ") : "none selected"}
            </span>
            <span>{contextPreviewOpen ? "Hide" : "Edit"}</span>
          </button>
          {contextPreviewOpen && (
            <div className="space-y-2 border-t border-surface-border p-2 text-[11px]">
              <div className="grid grid-cols-2 gap-1">
                <ContextToggle
                  label="Active file"
                  detail={activeFile ? activeFile.split(/[/\\]/).pop() ?? activeFile : "none"}
                  checked={contextOptions.activeFile}
                  onChange={(checked) => setContextOptions((prev) => ({ ...prev, activeFile: checked }))}
                />
                <ContextToggle
                  label="Selection"
                  detail={selectedText ? `${selectedText.length} chars` : "none"}
                  checked={contextOptions.selection}
                  onChange={(checked) => setContextOptions((prev) => ({ ...prev, selection: checked }))}
                />
                <ContextToggle
                  label="Open files"
                  detail={`${openFiles.length}`}
                  checked={contextOptions.openFiles}
                  onChange={(checked) => setContextOptions((prev) => ({ ...prev, openFiles: checked }))}
                />
                <ContextToggle
                  label="Problems"
                  detail={`${problems.length}`}
                  checked={contextOptions.problems}
                  onChange={(checked) => setContextOptions((prev) => ({ ...prev, problems: checked }))}
                />
                <ContextToggle
                  label="Failed run"
                  detail={`${failedTaskCount}`}
                  checked={contextOptions.failedTask}
                  onChange={(checked) => setContextOptions((prev) => ({ ...prev, failedTask: checked }))}
                />
                <ContextToggle
                  label="Terminal"
                  detail={`${terminalSessionCount}`}
                  checked={contextOptions.terminalOutput}
                  onChange={(checked) => setContextOptions((prev) => ({ ...prev, terminalOutput: checked }))}
                />
                <ContextToggle
                  label="Logs"
                  detail={`${warningLogCount}`}
                  checked={contextOptions.logs}
                  onChange={(checked) => setContextOptions((prev) => ({ ...prev, logs: checked }))}
                />
                <ContextToggle
                  label="Git diff"
                  detail={formatEstimateDetail(contextEstimate, "git_diff", "workspace")}
                  checked={contextOptions.gitDiff}
                  onChange={(checked) => setContextOptions((prev) => ({ ...prev, gitDiff: checked }))}
                />
                <ContextToggle
                  label="Project tree"
                  detail={formatEstimateDetail(contextEstimate, "project_tree", "summary")}
                  checked={contextOptions.projectTree}
                  onChange={(checked) => setContextOptions((prev) => ({ ...prev, projectTree: checked }))}
                />
              </div>
              {contextEstimate && (
                <div className="space-y-1 rounded border border-surface-border/60 bg-surface-panel/60 p-1.5">
                  {contextEstimate.sections.map((section) => (
                    <div key={section.id} className="flex items-center gap-2 text-[10px]">
                      <span className={`h-1.5 w-1.5 rounded-full ${section.included ? "bg-diff-add" : "bg-diff-remove"}`} />
                      <span className="min-w-0 flex-1 truncate text-surface-muted">{section.label}</span>
                      <span className="font-mono text-surface-text">{section.estimatedTokens.toLocaleString()} tok</span>
                      {(section.trimmed || section.excludedReason) && (
                        <span className="max-w-[140px] truncate text-diff-remove" title={section.excludedReason ?? "trimmed"}>
                          {section.excludedReason ?? "trimmed"}
                        </span>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        {/* 输入 + 按钮 */}
        <div className="flex gap-2">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={isSending ? undefined : handleKeyDown}
            placeholder={
              isSending
                ? "Agent is working…"
                : agentState === "waiting_user"
                ? "Review diffs or continue… (Shift+Enter for newline)"
                : agentState === "error"
                ? "An error occurred. Try again…"
                : ideMode === "plan"
                ? "Draft an SDD… (Plan mode, Shift+Enter for newline)"
                : `Ask Agent… (Mode: ${agentMode}, Shift+Enter for newline)`
            }
            disabled={isSending}
            rows={2}
            className="flex-1 bg-surface-base text-surface-text text-xs p-2 rounded border border-surface-border focus:border-accent-blue focus:outline-none resize-none placeholder-surface-muted disabled:opacity-50"
          />

          {/* 按钮区 — 根据状态显示不同按钮 */}
          {isSending ? (
            /* 工作中 → 停止按钮 */
            <button
              onClick={handleStop}
              className="px-3 bg-red-600/70 hover:bg-red-600 text-white rounded text-xs font-medium transition-colors self-end flex-shrink-0"
            >
              ■&nbsp;Stop
            </button>
          ) : agentState === "waiting_user" ? (
            /* 等待用户 → 蓝色继续按钮 */
            <button
              onClick={handleSend}
              disabled={!input.trim()}
              className="px-3 bg-accent-blue hover:bg-blue-700 text-white rounded text-xs font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed self-end flex-shrink-0"
            >
              ↩&nbsp;Send
            </button>
          ) : agentState === "done" ? (
            /* 完成 → 绿色继续按钮 */
            <button
              onClick={handleSend}
              disabled={!input.trim()}
              className="px-3 bg-green-600/70 hover:bg-green-600 text-white rounded text-xs font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed self-end flex-shrink-0"
            >
              ✓&nbsp;Continue
            </button>
          ) : agentState === "error" ? (
            /* 出错 → 重试按钮 */
            <button
              onClick={handleSend}
              disabled={!input.trim()}
              className="px-3 bg-red-600/70 hover:bg-red-600 text-white rounded text-xs font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed self-end flex-shrink-0"
            >
              ↻&nbsp;Retry
            </button>
          ) : (
            /* 空闲 → 发送按钮 */
            <button
              onClick={handleSend}
              disabled={!input.trim()}
              className="px-3 bg-accent-blue hover:bg-blue-700 text-white rounded text-xs font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed self-end flex-shrink-0"
            >
              ↑&nbsp;Send
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function buildGhostSuggestions(
  problems: ProblemEntry[],
  taskRuns: Record<string, ProjectTaskRunState>,
  logs: LogEntry[]
) {
  const suggestions = [];
  const firstError = problems.find((problem) => problem.severity === "error") ?? problems[0];
  if (firstError) {
    suggestions.push({
      id: `problem-${firstError.file}-${firstError.message}`,
      title: `Inspect ${firstError.file}`,
      detail: firstError.message,
      prompt: `Analyze this diagnostic and propose a focused fix. File: ${firstError.file}\nDiagnostic: ${firstError.message}`,
      source: "problems" as const,
      createdAt: Date.now(),
    });
  }
  const failedTask = Object.values(taskRuns).find((task) => task.status === "failed");
  if (failedTask) {
    suggestions.push({
      id: `task-${failedTask.command ?? "failed"}`,
      title: "Review failed task",
      detail: failedTask.command ?? "A project task failed",
      prompt: `Analyze the failed task output and propose the smallest fix. Command: ${failedTask.command ?? "unknown"}`,
      source: "tasks" as const,
      createdAt: Date.now(),
    });
  }
  const warnLog = logs.find((log) => log.level === "error" || log.level === "warn");
  if (warnLog) {
    suggestions.push({
      id: `log-${warnLog.message}`,
      title: "Inspect recent log",
      detail: warnLog.message,
      prompt: `Analyze this recent IDE log and suggest whether code or configuration needs attention:\n${warnLog.message}`,
      source: "logs" as const,
      createdAt: Date.now(),
    });
  }
  return suggestions.slice(0, 3);
}

function loadContextOptions(): ChatContextOptions {
  if (typeof window === "undefined") return DEFAULT_CONTEXT_OPTIONS;
  try {
    const workspacePath = localStorage.getItem("agent-ide-workspace-path") ?? "";
    const raw = localStorage.getItem(CONTEXT_OPTIONS_KEY);
    if (!raw) return DEFAULT_CONTEXT_OPTIONS;
    const parsed = JSON.parse(raw) as { workspacePath?: string; options?: Partial<ChatContextOptions> };
    if (parsed.workspacePath && workspacePath && parsed.workspacePath !== workspacePath) {
      return DEFAULT_CONTEXT_OPTIONS;
    }
    return { ...DEFAULT_CONTEXT_OPTIONS, ...(parsed.options ?? {}) };
  } catch {
    return DEFAULT_CONTEXT_OPTIONS;
  }
}

function persistContextOptions(options: ChatContextOptions) {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem(
      CONTEXT_OPTIONS_KEY,
      JSON.stringify({
        workspacePath: localStorage.getItem("agent-ide-workspace-path") ?? "",
        options,
      })
    );
  } catch {
    // Ignore persistence failures.
  }
}

function formatEstimateDetail(
  estimate: ContextEstimateResponse | null,
  sectionId: string,
  fallback: string
) {
  const section = estimate?.sections.find((item) => item.id === sectionId);
  if (!section) return fallback;
  if (!section.included) return "excluded";
  const suffix = section.trimmed ? " trimmed" : "";
  return `${section.estimatedTokens.toLocaleString()} tok${suffix}`;
}
