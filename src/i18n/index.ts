import { create } from "zustand";
import { EN, ZH, type MessageKey } from "./messages";

export type Locale = "en" | "zh";

const STORAGE_KEY = "agent-ide-locale";

/**
 * 把任意字符串收成一个合法 locale。
 *
 * 不用 `as Locale`：这个值来自 `localStorage` 和 `navigator.language`，两个都可能是上一个
 * 版本写的、或者根本不是我们认识的东西。断言只是让类型检查闭嘴，然后一个不存在的语言
 * 一路走到查表那一步，界面渲染出一堆 undefined。同 `normalizeAgentMode`。
 */
export function normalizeLocale(value: string | null | undefined): Locale {
  const raw = (value ?? "").trim().toLowerCase();
  if (raw.startsWith("zh")) return "zh";
  if (raw.startsWith("en")) return "en";
  return "en";
}

/** 没存过语言时按系统语言选：中文用户装完就是中文，不用先找一个开关 */
function detectLocale(): Locale {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) return normalizeLocale(stored);
  } catch {
    /* ignore */
  }
  if (typeof navigator !== "undefined") {
    return normalizeLocale(navigator.language ?? navigator.languages?.[0]);
  }
  return "en";
}

function applyLocale(locale: Locale) {
  try {
    localStorage.setItem(STORAGE_KEY, locale);
  } catch {
    /* ignore */
  }
  if (typeof document !== "undefined") {
    // `lang` 影响字体回退和断行：中文正文用西文的断行规则会在标点处断错行
    document.documentElement.setAttribute("lang", locale === "zh" ? "zh-CN" : "en");
  }
}

/** 查表并替换 `{name}` 占位符 */
export function translate(
  locale: Locale,
  key: MessageKey,
  params?: Record<string, string | number>
): string {
  const table = locale === "zh" ? ZH : EN;
  const template = table[key];
  if (!params) return template;
  return Object.entries(params).reduce(
    (text, [name, value]) => text.replaceAll(`{${name}}`, String(value)),
    template as string
  );
}

interface LocaleStore {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  toggleLocale: () => void;
}

export const useLocaleStore = create<LocaleStore>((set, get) => {
  const initial = detectLocale();
  applyLocale(initial);
  return {
    locale: initial,
    setLocale: (locale: Locale) => {
      applyLocale(locale);
      set({ locale });
    },
    // 两种语言之间用一次点击切换，不做下拉菜单：多一层菜单就多一步"去哪里找"
    toggleLocale: () => {
      const next: Locale = get().locale === "zh" ? "en" : "zh";
      applyLocale(next);
      set({ locale: next });
    },
  };
});

/** 组件里取翻译函数。语言变了会重渲染，因为它订阅的是 store。 */
export function useT() {
  const locale = useLocaleStore((state) => state.locale);
  return (key: MessageKey, params?: Record<string, string | number>) =>
    translate(locale, key, params);
}
