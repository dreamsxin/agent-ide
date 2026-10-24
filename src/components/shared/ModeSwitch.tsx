import { useT } from "../../i18n";
import type { AgentMode } from "../../types/agent";

const MODE_KEYS = [
  { key: "suggest", label: "mode.suggest", desc: "mode.suggest.desc" },
  { key: "auto", label: "mode.auto", desc: "mode.auto.desc" },
] as const satisfies readonly { key: AgentMode; label: string; desc: string }[];

interface ModeSwitchProps {
  mode: AgentMode;
  onChange: (mode: AgentMode) => void;
}

export default function ModeSwitch({ mode, onChange }: ModeSwitchProps) {
  const t = useT();
  return (
    <div
      className="flex bg-surface-base rounded p-0.5 border border-surface-border"
      role="radiogroup"
      aria-label={t("mode.group")}
    >
      {MODE_KEYS.map((m) => (
        <button
          key={m.key}
          role="radio"
          aria-checked={mode === m.key}
          onClick={() => onChange(m.key)}
          title={t(m.desc)}
          className={`px-3 py-1 text-xs rounded transition-colors ${
            mode === m.key
              ? "bg-accent-blue text-white"
              : "text-surface-muted hover:text-surface-text hover:bg-surface-border/50"
          }`}
        >
          {t(m.label)}
        </button>
      ))}
    </div>
  );
}
