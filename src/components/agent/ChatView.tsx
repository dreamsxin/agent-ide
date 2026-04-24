import { useState, useRef, useEffect } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import type { ChatMessage } from "../../types/agent";

export default function ChatView() {
  const [messages, setMessages] = useState<ChatMessage[]>([
    {
      id: "welcome",
      role: "system",
      content:
        "Welcome to Agent IDE. I'm your AI coding assistant. Try selecting code for quick actions, or ask me to build something.",
      timestamp: Date.now(),
    },
  ]);
  const [input, setInput] = useState("");
  const bottomRef = useRef<HTMLDivElement>(null);
  const agentState = useAgentStore((s) => s.state);
  const agentMode = useAgentStore((s) => s.mode);
  const isActing = agentState !== "idle" && agentState !== "done" && agentState !== "error";

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const handleSend = () => {
    if (!input.trim() || isActing) return;
    const userMsg: ChatMessage = {
      id: Date.now().toString(),
      role: "user",
      content: input.trim(),
      timestamp: Date.now(),
    };
    setMessages((prev) => [...prev, userMsg]);
    setInput("");

    // TODO: 对接 Tauri Agent IPC
    setTimeout(() => {
      const reply: ChatMessage = {
        id: (Date.now() + 1).toString(),
        role: "agent",
        content: `Received: "${userMsg.content}". Mode: ${agentMode}. I'll analyze this...`,
        timestamp: Date.now(),
      };
      setMessages((prev) => [...prev, reply]);
    }, 500);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  return (
    <div className="flex flex-col h-full">
      {/* 消息列表 */}
      <div className="flex-1 overflow-auto p-3 space-y-3">
        {messages.map((msg) => (
          <div
            key={msg.id}
            className={`animate-fade-in ${
              msg.role === "user" ? "flex justify-end" : "flex justify-start"
            }`}
          >
            <div
              className={`max-w-[90%] rounded-lg px-3 py-2 text-xs leading-relaxed ${
                msg.role === "user"
                  ? "bg-accent-blue text-white"
                  : msg.role === "system"
                  ? "bg-surface-border/30 text-surface-muted text-center w-full"
                  : "bg-surface-border/50 text-surface-text"
              }`}
            >
              {msg.content}
            </div>
          </div>
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
            placeholder={isActing ? "Agent is working..." : "Ask Agent... (Shift+Enter for newline)"}
            disabled={isActing}
            rows={2}
            className="flex-1 bg-surface-base text-surface-text text-xs p-2 rounded border border-surface-border focus:border-accent-blue focus:outline-none resize-none placeholder-surface-muted disabled:opacity-50"
          />
          <button
            onClick={handleSend}
            disabled={!input.trim() || isActing}
            className="px-3 bg-accent-blue hover:bg-blue-700 text-white rounded text-xs transition-colors disabled:opacity-40 disabled:cursor-not-allowed self-end"
          >
            Send
          </button>
        </div>
      </div>
    </div>
  );
}
