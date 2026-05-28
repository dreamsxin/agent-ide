import { useMemo } from "react";
import { useEditorStore } from "../../stores/useEditorStore";
import { useAgentStore } from "../../stores/useAgentStore";
import { useLayoutStore } from "../../stores/useLayoutStore";
import { useMonacoContext } from "./MonacoContext";
import {
  AGENT_QUICK_ACTIONS,
  buildActionPrompt,
  type AgentQuickActionKey,
} from "../../utils/agentActions";

/**
 * Floating toolbar above selected text: [Explain | Fix | Refactor | Optimize]
 */
export default function QuickActions() {
  const selectedText = useEditorStore((s) => s.selectedText);
  const selectedRange = useEditorStore((s) => s.selectedRange);
  const activeFile = useEditorStore((s) => s.activeFile);
  const fileContents = useEditorStore((s) => s.fileContents);
  const sendPrompt = useAgentStore((s) => s.sendPrompt);
  const addMessage = useAgentStore((s) => s.addMessage);
  const agentState = useAgentStore((s) => s.state);
  const rightVisible = useLayoutStore((s) => s.rightVisible);
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel);
  const { editor, monaco } = useMonacoContext();

  // Calculate toolbar pixel position
  const position = useMemo(() => {
    if (!editor || !monaco || !selectedRange || !selectedText) return null;

    try {
      const top = editor.getTopForLineNumber(selectedRange.startLine);
      const editorLayout = editor.getLayoutInfo();

      return {
        top: top - 40,
        left: editorLayout.contentLeft + editorLayout.contentWidth / 2,
      };
    } catch {
      return null;
    }
  }, [selectedText, selectedRange, editor, monaco]);

  const isAgentBusy =
    agentState !== "idle" &&
    agentState !== "done" &&
    agentState !== "error" &&
    agentState !== "waiting_user";

  const runQuickAction = async (action: AgentQuickActionKey) => {
    if (!selectedText || isAgentBusy) return;
    if (!rightVisible) {
      toggleRightPanel();
    }

    const prompt = buildActionPrompt(
      action,
      selectedText,
      activeFile,
      selectedRange?.startLine,
      selectedRange?.endLine
    );

    addMessage({
      id: `quick-${Date.now()}`,
      role: "user",
      content: prompt,
      timestamp: Date.now(),
    });

    await sendPrompt({
      prompt,
      contextFiles: activeFile ? [activeFile] : [],
      activeFile: activeFile ?? undefined,
      activeFileContent: activeFile ? fileContents[activeFile] : undefined,
      selection: selectedText,
      ideMode: "code",
    });
  };

  if (!selectedText || !position) return null;

  return (
    <div
      className="absolute z-20 animate-fade-in pointer-events-auto"
      style={{
        top: position.top,
        left: position.left,
        transform: "translateX(-50%)",
      }}
    >
      <div className="flex gap-0.5 bg-surface-panel border border-surface-border rounded-lg p-1 shadow-lg">
        {AGENT_QUICK_ACTIONS.map((action) => (
          <button
            key={action.key}
            onClick={(e) => {
              e.stopPropagation();
              void runQuickAction(action.key as AgentQuickActionKey);
            }}
            disabled={isAgentBusy}
            className="flex items-center gap-1 px-2 py-1 text-xs text-surface-muted hover:text-surface-text hover:bg-surface-border/50 rounded transition-colors whitespace-nowrap"
          >
            <span>{action.icon}</span>
            <span>{action.label}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
