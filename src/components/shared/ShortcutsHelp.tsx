import { useEffect } from "react";
import type { Shortcut } from "../../hooks/useShortcuts";
import { useT } from "../../i18n";

interface ShortcutsHelpProps {
  shortcuts: Shortcut[];
  visible: boolean;
  onClose: () => void;
}

export default function ShortcutsHelp({ shortcuts, visible, onClose }: ShortcutsHelpProps) {
  const t = useT();
  // Esc 关闭。此前只能点背景或再按一次 F1 —— 一个讲快捷键的弹窗自己不响应 Esc
  // 是最难自圆其说的一处。
  useEffect(() => {
    if (!visible) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [visible, onClose]);

  if (!visible) return null;

  // Group shortcuts
  // Map 的键用分组 id 的联合类型，不用 string：`t()` 只接受真实存在的键，
  // 键类型松成 string 之后 `shortcut.group.${group}` 就推不出是合法键了
  const grouped = new Map<Shortcut["group"], Shortcut[]>();
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
        role="dialog"
        aria-modal="true"
        aria-label={t("shortcut.title")}
        className="bg-surface-panel border border-surface-border rounded-lg shadow-2xl max-w-lg w-full mx-4 max-h-[80vh] overflow-hidden animate-slide-up"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-surface-border">
          <h2 className="text-sm font-semibold text-surface-text">{t("shortcut.title")}</h2>
          <button
            onClick={onClose}
            aria-label={t("shortcut.close")}
            title={t("shortcut.close.title")}
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
                {t(`shortcut.group.${group}`)}
              </div>
              {items.map((s) => (
                <div
                  key={s.id}
                  className="flex items-center justify-between px-2 py-1.5 rounded hover:bg-surface-border/20 text-xs"
                >
                  <span className="text-surface-text">{t(s.labelKey)}</span>
                  <kbd className="px-2 py-0.5 bg-surface-base border border-surface-border rounded text-[10px] text-surface-muted font-mono">
                    {s.keys}
                  </kbd>
                </div>
              ))}
            </div>
          ))}
        </div>

        {/* Footer。F1 作为参数插进去，不再把句子拆成"Press" + <kbd> + "to toggle…"：
            那种拼法在中文里语序不对，而且两半分别翻译谁都读不懂自己在翻什么。 */}
        <div className="px-4 py-2 border-t border-surface-border text-[10px] text-surface-muted">
          {t("shortcut.footer", { key: "F1" })}
        </div>
      </div>
    </div>
  );
}
