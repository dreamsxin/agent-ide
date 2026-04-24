import { useAgentStore } from "../../stores/useAgentStore";
import type { AgentRole } from "../../types/agent";

const ROLES: { id: AgentRole; label: string; desc: string; icon: string }[] = [
  { id: "architect", label: "Architect", desc: "Design architecture & plan tasks", icon: "🏗" },
  { id: "coder", label: "Coder", desc: "Write and modify code", icon: "💻" },
  { id: "tester", label: "Tester", desc: "Write and run tests", icon: "🧪" },
  { id: "reviewer", label: "Reviewer", desc: "Review code quality & security", icon: "🔍" },
];

export default function AgentSelector() {
  const mode = useAgentStore((s) => s.mode);
  const state = useAgentStore((s) => s.state);
  const isRunning = state !== "idle" && state !== "done" && state !== "error";

  return (
    <div className="p-3 text-xs">
      <div className="text-surface-muted mb-2 font-semibold tracking-wide">
        Agent Roles
      </div>

      <div className="space-y-1.5">
        {ROLES.map((role) => (
          <div
            key={role.id}
            className={`flex items-start gap-2 p-2 rounded border transition-colors ${
              isRunning
                ? "border-surface-border/30 opacity-50 cursor-not-allowed"
                : "border-surface-border cursor-pointer hover:border-accent-blue/40 hover:bg-surface-border/10"
            }`}
          >
            <span className="text-sm mt-0.5 flex-shrink-0">{role.icon}</span>
            <div className="min-w-0">
              <div className="font-medium text-surface-text">{role.label}</div>
              <div className="text-[10px] text-surface-muted leading-tight">
                {role.desc}
              </div>
            </div>
            <div className="ml-auto flex-shrink-0">
              <span className="inline-block w-2 h-2 rounded-full bg-accent-green" />
            </div>
          </div>
        ))}
      </div>

      <div className="mt-3 pt-2 border-t border-surface-border">
        <div className="text-surface-muted text-[10px]">
          Pipeline: Design → Implement → Test → Review
        </div>
        <div className="text-surface-muted text-[10px] mt-1">
          Mode: <span className="text-accent-blue">{mode}</span>
        </div>
      </div>
    </div>
  );
}
