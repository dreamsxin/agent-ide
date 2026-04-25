import { create } from "zustand";

export type Theme = "dark" | "light";

interface ThemeStore {
  theme: Theme;
  toggleTheme: () => void;
  setTheme: (t: Theme) => void;
}

/** 从 localStorage 恢复主题 */
function loadTheme(): Theme {
  try {
    const stored = localStorage.getItem("agent-ide-theme");
    if (stored === "light" || stored === "dark") return stored;
  } catch { /* ignore */ }
  return "dark";
}

/** 保存主题到 localStorage 并应用到 DOM */
function applyTheme(theme: Theme) {
  try {
    localStorage.setItem("agent-ide-theme", theme);
  } catch { /* ignore */ }
  document.documentElement.setAttribute("data-theme", theme);
}

// 初始化
applyTheme(loadTheme());

export const useThemeStore = create<ThemeStore>((set) => ({
  theme: loadTheme(),

  toggleTheme: () =>
    set((s) => {
      const next: Theme = s.theme === "dark" ? "light" : "dark";
      applyTheme(next);
      return { theme: next };
    }),

  setTheme: (theme: Theme) => {
    applyTheme(theme);
    set({ theme });
  },
}));
