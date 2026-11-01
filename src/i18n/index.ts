import i18n, { type BackendModule, type ResourceKey } from "i18next";
import { initReactI18next } from "react-i18next";

// English is the fallback language and is always bundled; the other locales
// are separate chunks loaded on demand (FE-ARCH-08 / PERF-15), so startup
// parses one or two locale files instead of all four.
import en from "./locales/en.json";
import {
  DEFAULT_LANGUAGE,
  getInitialLanguage,
  syncDocumentLanguage,
  type Language,
} from "./language";

export {
  DEFAULT_LANGUAGE,
  getInitialLanguage,
  mapLocaleToLanguage,
  syncDocumentLanguage,
} from "./language";
export type { Language } from "./language";

type LocaleModule = { default: ResourceKey };

const LAZY_LOCALES: Record<
  Exclude<Language, "en">,
  () => Promise<LocaleModule>
> = {
  ja: () => import("./locales/ja.json"),
  zh: () => import("./locales/zh.json"),
  "zh-TW": () => import("./locales/zh-TW.json"),
};

/**
 * i18next backend that resolves a locale to its lazily imported JSON chunk.
 * Unknown languages resolve to an empty bundle (i18next then falls back to
 * English).
 */
export const lazyLocaleBackend: BackendModule = {
  type: "backend",
  init() {},
  read(language, _namespace, callback) {
    if (language === "en") {
      callback(null, en);
      return;
    }
    const load = (
      LAZY_LOCALES as Record<string, (() => Promise<LocaleModule>) | undefined>
    )[language];
    if (!load) {
      callback(null, {});
      return;
    }
    load().then(
      (module) => callback(null, module.default),
      (error: unknown) =>
        callback(
          error instanceof Error ? error : new Error(String(error)),
          null,
        ),
    );
  },
};

// 语言切换时同步 <html lang>（init 完成时也会触发一次）
i18n.on("languageChanged", syncDocumentLanguage);

/**
 * Resolves once the initial language's resources are loaded. Render after
 * awaiting it to avoid a flash of fallback (English) text.
 */
export const i18nReady = i18n
  .use(lazyLocaleBackend)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
    },
    // Bundled `en` plus backend-loaded others.
    partialBundledLanguages: true,
    lng: getInitialLanguage(), // 根据本地存储或系统语言选择默认语言
    fallbackLng: DEFAULT_LANGUAGE, // 如果缺少翻译则退回英文

    interpolation: {
      escapeValue: false, // React 已经默认转义
    },

    react: {
      // Keep rendering while a newly selected locale chunk loads.
      useSuspense: false,
    },

    // 开发模式下显示调试信息
    debug: false,
  })
  .catch((error: unknown) => {
    console.error("[i18n] Failed to initialise translations", error);
  });

export default i18n;
