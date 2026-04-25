import type { Shortcut } from "../../hooks/useShortcuts";

interface ShortcutsHelpProps {
  shortcuts: Shortcut[];
  visible: boolean;
  onClose: () => void;
}

const GROUP_LABELS: Record<string, string> = {
  Panels: "Panel",
  Git: "Git",
  Navigation: "Navigation",
  Editor: "Editor",
  General: "General",
};

export default function ShortcutsHelp({ shortcuts, visible, onClose }: ShortcutsHelpProps) {
  if (!visible) return null;

  // Group shortcuts
  const grouped = new Map<string, Shortcut[]>();
  for (const s of shortcuts) {
    const list = grouped.get(s.group) || [];
    list.push(s);
    grouped.set(s.group, list);
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 animate-fade-in"
      onClick={onClose}
    >
      <div
        className="bg-surface-panel border border-surface-border rounded-lg shadow-2xl max-w-lg w-full mx-4 max-h-[80vh] overflow-hidden animate-slide-up"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-surface-border">
          <h2 className="text-sm font-semibold text-surface-text">Keyboard Shortcuts</h2>
          <button
            onClick={onClose}
            className="text-surface-muted hover:text-surface-text text-lg leading-none px-1"
          >
            ✕
          </button>
        </div>

        {/* Content */}
        <div className="overflow-auto p-2 max-h-[60vh]">
          {Array.from(grouped.entries()).map(([group, items]) => (
            <div key={group} className="mb-3">
              <div className="text-[10px] font-semibold text-surface-muted uppercase tracking-wider px-2 py-1">
                {GROUP_LABELS[group] ?? group}
              </div>
              {items.map((s) => (
                <div
                  key={s.id}
                  className="flex items-center justify-between px-2 py-1.5 rounded hover:bg-surface-border/20 text-xs"
                >
                  <span className="text-surface-text">{s.label}</span>
                  <kbd className="px-2 py-0.5 bg-surface-base border border-surface-border rounded text-[10px] text-surface-muted font-mono">
                    {s.keys.replace("Ctrl", "Ctrl").replace("Shift", "Shift")}
                  </kbd>
                </div>
              ))}
            </div>
          ))}
        </div>

        {/* Footer */}
        <div className="px-4 py-2 border-t border-surface-border text-[10px] text-surface-muted">
          Press <kbd className="px-1 py-0.5 bg-surface-base border border-surface-border rounded text-[9px]">F1</kbd> to toggle this panel
        </div>
      </div>
    </div>
  );
}
