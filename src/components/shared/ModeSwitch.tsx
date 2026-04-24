import type { AgentMode } from "../../types/agent";

const modes: { key: AgentMode; label: string; desc: string }[] = [
  { key: "suggest", label: "Suggest", desc: "仅建议" },
  { key: "edit", label: "Edit", desc: "可编辑" },
  { key: "auto", label: "Auto", desc: "全自动" },
];

interface ModeSwitchProps {
  mode: AgentMode;
  onChange: (mode: AgentMode) => void;
}

export default function ModeSwitch({ mode, onChange }: ModeSwitchProps) {
  return (
    <div
      className="flex bg-surface-base rounded p-0.5 border border-surface-border"
      role="radiogroup"
      aria-label="Agent mode"
    >
      {modes.map((m) => (
        <button
          key={m.key}
          role="radio"
          aria-checked={mode === m.key}
          onClick={() => onChange(m.key)}
          title={m.desc}
          className={`px-3 py-1 text-xs rounded transition-colors ${
            mode === m.key
              ? "bg-accent-blue text-white"
              : "text-surface-muted hover:text-surface-text hover:bg-surface-border/50"
          }`}
        >
          {m.label}
        </button>
      ))}
    </div>
  );
}
