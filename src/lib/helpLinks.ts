/**
 * In-app help links into the user manual (DOCS-16).
 *
 * One map from help topic to manual page, so moving a page in
 * docs/user-manual/ is one edit here (tests/lib/helpLinks.test.ts checks that
 * every page exists). The manual is published in en, zh and ja; zh-TW reads
 * the zh pages, and a page missing in a language falls back to English.
 */

export const MANUAL_BASE_URL =
  "https://github.com/farion1231/cc-switch/blob/main/docs/user-manual";

export const HELP_TOPICS = {
  manual: "README.md",
  "getting-started": "1-getting-started/1.4-quickstart.md",
  settings: "1-getting-started/1.5-settings.md",
  projects: "1-getting-started/1.6-projects.md",
  "providers.add": "2-providers/2.1-add.md",
  "providers.switch": "2-providers/2.2-switch.md",
  "providers.usage": "2-providers/2.5-usage-query.md",
  "providers.claude-desktop": "2-providers/2.6-claude-desktop.md",
  mcp: "3-extensions/3.1-mcp.md",
  prompts: "3-extensions/3.2-prompts.md",
  skills: "3-extensions/3.3-skills.md",
  sessions: "3-extensions/3.4-sessions.md",
  "proxy.service": "4-proxy/4.1-service.md",
  "proxy.routing": "4-proxy/4.2-routing.md",
  "proxy.failover": "4-proxy/4.3-failover.md",
  "proxy.usage": "4-proxy/4.4-usage.md",
  "proxy.model-test": "4-proxy/4.5-model-test.md",
  "faq.config-files": "5-faq/5.1-config-files.md",
  "faq.deeplink": "5-faq/5.3-deeplink.md",
  "faq.env-conflict": "5-faq/5.4-env-conflict.md",
  "faq.command-line": "5-faq/5.5-command-line.md",
  troubleshooting: "5-faq/5.6-troubleshooting.md",
} as const;

export type HelpTopic = keyof typeof HELP_TOPICS;
export type ManualLanguage = "en" | "zh" | "ja";

/** Pages that exist only in the English manual. */
export const ENGLISH_ONLY_PAGES: ReadonlySet<string> = new Set([
  HELP_TOPICS["faq.command-line"],
]);

/** The manual language for a UI language (`ja`, `zh`/`zh-TW` -> zh, else en). */
export function manualLanguage(uiLanguage: string | undefined): ManualLanguage {
  const lang = (uiLanguage ?? "").toLowerCase();
  if (lang.startsWith("ja")) return "ja";
  if (lang.startsWith("zh")) return "zh";
  return "en";
}

/** Absolute URL of a help topic in the active language, English as fallback. */
export function helpUrl(
  topic: HelpTopic,
  uiLanguage: string | undefined,
  anchor?: string,
): string {
  const page = HELP_TOPICS[topic];
  const lang = ENGLISH_ONLY_PAGES.has(page) ? "en" : manualLanguage(uiLanguage);
  const hash = anchor ? `#${encodeURIComponent(anchor)}` : "";
  return `${MANUAL_BASE_URL}/${lang}/${page}${hash}`;
}
