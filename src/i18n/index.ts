import i18n, { type BackendModule, type ResourceKey } from "i18next";
import { initReactI18next } from "react-i18next";

import { safeStorageGet } from "@/lib/storage";

type Language = "zh" | "zh-TW" | "en" | "ja";

/**
 * 语言包按需加载：每种语言单独成块，只拉取当前语言（及回退语言 en），
 * 不再把四份 JSON 全部打进首屏包。
 */
export const LOCALE_LOADERS: Record<
  Language,
  () => Promise<{ default: ResourceKey }>
> = {
  en: () => import("./locales/en.json"),
  ja: () => import("./locales/ja.json"),
  zh: () => import("./locales/zh.json"),
  "zh-TW": () => import("./locales/zh-TW.json"),
};

const isLanguage = (lng: string): lng is Language =>
  Object.prototype.hasOwnProperty.call(LOCALE_LOADERS, lng);

export const lazyLocaleBackend: BackendModule = {
  type: "backend",
  init() {},
  read(language, _namespace, callback) {
    if (!isLanguage(language)) {
      callback(null, {});
      return;
    }
    LOCALE_LOADERS[language]()
      .then((mod) => callback(null, mod.default))
      .catch((error: unknown) => {
        console.error(`[i18n] Failed to load locale "${language}"`, error);
        callback(error as Error, false);
      });
  },
};

const DEFAULT_LANGUAGE: Language = "zh";

const getInitialLanguage = (): Language => {
  if (typeof window !== "undefined") {
    try {
      const stored = safeStorageGet("language");
      if (
        stored === "zh" ||
        stored === "zh-TW" ||
        stored === "en" ||
        stored === "ja"
      ) {
        return stored;
      }
    } catch (error) {
      console.warn("[i18n] Failed to read stored language preference", error);
    }
  }

  const navigatorLang =
    typeof navigator !== "undefined"
      ? (navigator.language?.toLowerCase() ??
        navigator.languages?.[0]?.toLowerCase())
      : undefined;

  if (navigatorLang === "zh") {
    return "zh";
  }

  if (
    navigatorLang?.startsWith("zh-tw") ||
    navigatorLang?.startsWith("zh-hk") ||
    navigatorLang?.startsWith("zh-mo") ||
    navigatorLang?.startsWith("zh-hant")
  ) {
    return "zh-TW";
  }

  if (navigatorLang?.startsWith("zh")) {
    return "zh";
  }

  if (navigatorLang?.startsWith("ja")) {
    return "ja";
  }

  if (navigatorLang?.startsWith("en")) {
    return "en";
  }

  return DEFAULT_LANGUAGE;
};

/** 初始语言包加载完成（成功或失败）后 resolve，渲染前等待以避免闪现翻译键 */
export const i18nReady = i18n
  .use(lazyLocaleBackend)
  .use(initReactI18next)
  .init({
    lng: getInitialLanguage(), // 根据本地存储或系统语言选择默认语言
    fallbackLng: "en", // 如果缺少中文翻译则退回英文
    supportedLngs: Object.keys(LOCALE_LOADERS),

    interpolation: {
      escapeValue: false, // React 已经默认转义
    },

    react: {
      // 切换语言时语言包异步到达，由 i18nReady / languageChanged 驱动重渲染
      useSuspense: false,
    },

    // 开发模式下显示调试信息
    debug: false,
  })
  .then(
    () => undefined,
    (error: unknown) => {
      console.error("[i18n] Initialization failed", error);
    },
  );

export default i18n;
