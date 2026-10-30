export type Language = "zh" | "zh-TW" | "en" | "ja";

/** 未知系统区域（de/fr/ko/es 等）回退到英文，与托盘及后端保持一致。 */
export const DEFAULT_LANGUAGE: Language = "en";

const isLanguage = (value: unknown): value is Language =>
  value === "zh" || value === "zh-TW" || value === "en" || value === "ja";

/**
 * 将系统/浏览器区域标识映射为应用支持的语言。
 * 繁中（zh-TW/HK/MO/Hant）→ zh-TW，其余 zh → zh，ja → ja，其余 → en。
 */
export const mapLocaleToLanguage = (
  locale: string | null | undefined,
): Language => {
  const lang = locale?.toLowerCase().replace(/_/g, "-");
  if (!lang) return DEFAULT_LANGUAGE;

  if (lang === "zh") return "zh";
  if (
    lang.startsWith("zh-tw") ||
    lang.startsWith("zh-hk") ||
    lang.startsWith("zh-mo") ||
    lang.startsWith("zh-hant")
  ) {
    return "zh-TW";
  }
  if (lang.startsWith("zh")) return "zh";
  if (lang.startsWith("ja")) return "ja";
  return DEFAULT_LANGUAGE;
};

const readStoredLanguage = (): Language | undefined => {
  if (typeof window === "undefined") return undefined;
  try {
    const stored = window.localStorage.getItem("language");
    return isLanguage(stored) ? stored : undefined;
  } catch (error) {
    console.warn("[i18n] Failed to read stored language preference", error);
    return undefined;
  }
};

const readNavigatorLocale = (): string | undefined => {
  if (typeof navigator === "undefined") return undefined;
  return navigator.language ?? navigator.languages?.[0];
};

/**
 * 首次启动语言：优先本地存储的用户选择；否则按系统区域推断。
 * 传入 `locale` 时跳过本地存储，仅用于映射（便于测试）。
 */
export const getInitialLanguage = (locale?: string): Language => {
  if (locale !== undefined) return mapLocaleToLanguage(locale);
  return readStoredLanguage() ?? mapLocaleToLanguage(readNavigatorLocale());
};

/** 同步 `<html lang>`，便于读屏软件和字体选择识别当前语言。 */
export const syncDocumentLanguage = (language: string | undefined): void => {
  if (typeof document === "undefined" || !language) return;
  document.documentElement.lang = language;
};
