import type { AgentState } from "../../types/agent";

const stateConfig: Record<AgentState, { color: string; label: string; animate: boolean }> = {
  idle: { color: "bg-gray-500", label: "Idle", animate: false },
  thinking: { color: "bg-purple-500", label: "Thinking", animate: true },
  planning: { color: "bg-yellow-500", label: "Planning", animate: true },
  acting: { color: "bg-blue-500", label: "Acting", animate: true },
  reviewing: { color: "bg-orange-500", label: "Reviewing", animate: true },
  waiting_user: { color: "bg-cyan-500", label: "Waiting", animate: true },
  done: { color: "bg-green-500", label: "Done", animate: false },
  error: { color: "bg-red-500", label: "Error", animate: false },
};

interface StatusDotProps {
  state: AgentState;
  showLabel?: boolean;
}

export default function StatusDot({ state, showLabel = true }: StatusDotProps) {
  const config = stateConfig[state] ?? stateConfig.idle;

  return (
    <div className="flex items-center gap-1.5">
      <span
        className={`inline-block w-2 h-2 rounded-full ${config.color} ${
          config.animate ? "animate-pulse-dot" : ""
        }`}
      />
      {showLabel && (
        <span className="text-xs text-surface-muted">{config.label}</span>
      )}
    </div>
  );
}
