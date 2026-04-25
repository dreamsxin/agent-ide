import { useState, useRef, useEffect, useCallback } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import { useEditorStore } from "../../stores/useEditorStore";
import { marked } from "marked";

// 配置 marked
marked.setOptions({
  breaks: true,   // 换行 → <br>
  gfm: true,      // GitHub Flavored Markdown
});

/** 将 markdown 渲染为 HTML，支持流式不完整代码块 */
function renderMarkdown(raw: string): string {
  try {
    return marked.parse(raw) as string;
  } catch {
    return escapeHtml(raw);
  }
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

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
          <div
            className="markdown-body"
            dangerouslySetInnerHTML={{ __html: renderMarkdown(msg.content) }}
          />
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

export default function ChatView() {
  const messages = useAgentStore((s) => s.messages);
  const addMessage = useAgentStore((s) => s.addMessage);
  const updateMessage = useAgentStore((s) => s.updateMessage);
  const [input, setInput] = useState("");
  const bottomRef = useRef<HTMLDivElement>(null);

  const agentState = useAgentStore((s) => s.state);
  const agentMode = useAgentStore((s) => s.mode);
  const streamContent = useAgentStore((s) => s.streamContent);
  const isStreaming = useAgentStore((s) => s.isStreaming);
  const sendPrompt = useAgentStore((s) => s.sendPrompt);
  const stopAgent = useAgentStore((s) => s.stopAgent);

  const activeFile = useEditorStore((s) => s.activeFile);
  const fileContents = useEditorStore((s) => s.fileContents);
  const selectedText = useEditorStore((s) => s.selectedText);

  const isActing =
    agentState !== "idle" && agentState !== "done" && agentState !== "error" && agentState !== "waiting_user";

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
      // 新的流式消息
      const newId = `stream-${Date.now()}`;
      streamingMsgId.current = newId;
      addMessage({
        id: newId,
        role: "agent",
        content: streamContent,
        timestamp: Date.now(),
      });
    }
  }, [streamContent, isStreaming]);

  // 流结束时固化消息
  useEffect(() => {
    if (!isStreaming && streamingMsgId.current) {
      streamingMsgId.current = null;
    }
  }, [isStreaming]);

  const buildContext = useCallback(() => {
    return {
      activeFile: activeFile ?? undefined,
      activeFileContent: activeFile ? fileContents[activeFile] : undefined,
      selection: selectedText ?? undefined,
    };
  }, [activeFile, fileContents, selectedText]);

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

    // 调用 Agent
    const ctx = buildContext();
    await sendPrompt({
      prompt: content,
      ...ctx,
    });

    // Agent 完成后不添加额外消息（流式消息已存在）
  }, [input, isActing, sendPrompt, buildContext, addMessage]);

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
        <div ref={bottomRef} />
      </div>

      {/* 输入区 */}
      <div className="p-2 border-t border-surface-border">
        <div className="flex gap-2">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={
              isActing
                ? "Agent is working..."
                : `Ask Agent... (Mode: ${agentMode}, Shift+Enter for newline)`
            }
            disabled={isActing}
            rows={2}
            className="flex-1 bg-surface-base text-surface-text text-xs p-2 rounded border border-surface-border focus:border-accent-blue focus:outline-none resize-none placeholder-surface-muted disabled:opacity-50"
          />
          {isActing ? (
            <button
              onClick={handleStop}
              className="px-3 bg-red-600/70 hover:bg-red-600 text-white rounded text-xs transition-colors self-end"
            >
              Stop
            </button>
          ) : (
            <button
              onClick={handleSend}
              disabled={!input.trim()}
              className="px-3 bg-accent-blue hover:bg-blue-700 text-white rounded text-xs transition-colors disabled:opacity-40 disabled:cursor-not-allowed self-end"
            >
              Send
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
