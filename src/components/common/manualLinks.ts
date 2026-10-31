/**
 * Links into the user manual on GitHub (docs/user-manual/{en,zh,ja}).
 * Used where the UI itself cannot help, e.g. the fatal startup dialog
 * (DOCS-07).
 */
const MANUAL_BASE =
  "https://github.com/farion1231/cc-switch/blob/main/docs/user-manual";

/** Manual language folder for a UI language (zh-TW reads the zh manual). */
export function manualLanguage(
  language: string | undefined,
): "en" | "zh" | "ja" {
  const lang = (language ?? "").toLowerCase();
  if (lang.startsWith("zh")) return "zh";
  if (lang.startsWith("ja")) return "ja";
  return "en";
}

/** GitHub heading anchors of "Manual Restore When the App Does Not Start". */
const MANUAL_RESTORE_ANCHORS = {
  en: "manual-restore-when-the-app-does-not-start",
  zh: "应用无法启动时手动恢复",
  ja: "アプリが起動しないときの手動復元",
} as const;

/** The step-by-step manual database restore section, in the UI language. */
export function manualRestoreUrl(language: string | undefined): string {
  const lang = manualLanguage(language);
  return `${MANUAL_BASE}/${lang}/5-faq/5.1-config-files.md#${encodeURIComponent(
    MANUAL_RESTORE_ANCHORS[lang],
  )}`;
}
