/**
 * Searchable settings registry (FE-UI-38). Each entry names the tab (and, for
 * accordion tabs, the section) that holds a setting, plus the i18n key of the
 * title shown there; the title is also how the page finds the control to
 * focus (a toggle's aria-label or a section heading).
 */

export type SettingsTab =
  | "general"
  | "proxy"
  | "auth"
  | "advanced"
  | "usage"
  | "about";

export interface SettingsSearchEntry {
  id: string;
  tab: SettingsTab;
  /** Accordion item to expand (Proxy and Advanced tabs). */
  section?: string;
  /** i18n key of the title rendered on the page. */
  titleKey: string;
  /** English title, used when the key is missing. */
  defaultTitle: string;
  /** Extra English search terms. */
  keywords: string[];
}

export const SETTINGS_SEARCH_INDEX: readonly SettingsSearchEntry[] = [
  // General
  {
    id: "language",
    tab: "general",
    titleKey: "settings.language",
    defaultTitle: "Language",
    keywords: ["locale", "translation", "chinese", "japanese", "english"],
  },
  {
    id: "theme",
    tab: "general",
    titleKey: "settings.theme",
    defaultTitle: "Theme",
    keywords: ["dark", "light", "appearance", "color"],
  },
  {
    id: "appVisibility",
    tab: "general",
    titleKey: "settings.appVisibility.title",
    defaultTitle: "App visibility",
    keywords: ["hide", "show", "apps", "tabs"],
  },
  {
    id: "skillStorage",
    tab: "general",
    titleKey: "settings.skillStorage.title",
    defaultTitle: "Skill storage location",
    keywords: ["skills", "folder", "migrate"],
  },
  {
    id: "skillSync",
    tab: "general",
    titleKey: "settings.skillSync.title",
    defaultTitle: "Skill sync method",
    keywords: ["skills", "symlink", "copy"],
  },
  {
    id: "windowBehavior",
    tab: "general",
    titleKey: "settings.windowBehavior",
    defaultTitle: "Window behavior",
    keywords: ["window"],
  },
  {
    id: "launchOnStartup",
    tab: "general",
    titleKey: "settings.launchOnStartup",
    defaultTitle: "Launch on startup",
    keywords: ["autostart", "login", "boot"],
  },
  {
    id: "enableClaudePluginIntegration",
    tab: "general",
    titleKey: "settings.enableClaudePluginIntegration",
    defaultTitle: "Claude plugin integration",
    keywords: ["vscode", "plugin", "extension"],
  },
  {
    id: "skipClaudeOnboarding",
    tab: "general",
    titleKey: "settings.skipClaudeOnboarding",
    defaultTitle: "Skip Claude Code onboarding",
    keywords: ["onboarding", "first run", "welcome"],
  },
  {
    id: "minimizeToTray",
    tab: "general",
    titleKey: "settings.minimizeToTray",
    defaultTitle: "Minimize to tray on close",
    keywords: ["tray", "close", "menu bar"],
  },
  {
    id: "notifications",
    tab: "general",
    titleKey: "settings.notifications.title",
    defaultTitle: "Notifications",
    keywords: ["alert", "toast", "system notification"],
  },
  {
    id: "terminal",
    tab: "general",
    titleKey: "settings.terminal.title",
    defaultTitle: "Terminal",
    keywords: ["shell", "iterm", "console"],
  },
  // Proxy
  {
    id: "localProxy",
    tab: "proxy",
    section: "proxy",
    titleKey: "settings.advanced.proxy.title",
    defaultTitle: "Local proxy",
    keywords: ["routing", "port", "takeover", "listen", "logging"],
  },
  {
    id: "failover",
    tab: "proxy",
    section: "failover",
    titleKey: "settings.advanced.failover.title",
    defaultTitle: "Auto failover",
    keywords: ["queue", "circuit breaker", "fallback", "retry"],
  },
  {
    id: "rectifier",
    tab: "proxy",
    section: "rectifier",
    titleKey: "settings.advanced.rectifier.title",
    defaultTitle: "Rectifier",
    keywords: ["thinking", "signature", "media", "heuristic", "budget"],
  },
  {
    id: "globalProxy",
    tab: "proxy",
    section: "globalProxy",
    titleKey: "settings.advanced.globalProxy.title",
    defaultTitle: "Global outbound proxy",
    keywords: ["http proxy", "socks", "outbound", "network"],
  },
  // Auth
  {
    id: "auth",
    tab: "auth",
    titleKey: "settings.tabAuth",
    defaultTitle: "Authentication",
    keywords: ["login", "oauth", "copilot", "codex", "account"],
  },
  // Advanced
  {
    id: "configDir",
    tab: "advanced",
    section: "directory",
    titleKey: "settings.advanced.configDir.title",
    defaultTitle: "Configuration directories",
    keywords: ["directory", "folder", "path", "wsl"],
  },
  {
    id: "data",
    tab: "advanced",
    section: "data",
    titleKey: "settings.advanced.data.title",
    defaultTitle: "Data management",
    keywords: [
      "import",
      "export",
      "retention",
      "request logs",
      "vault",
      "secrets",
      "keychain",
    ],
  },
  {
    id: "backup",
    tab: "advanced",
    section: "backup",
    titleKey: "settings.advanced.backup.title",
    defaultTitle: "Backup & Restore",
    keywords: ["backup", "restore", "snapshot", "database"],
  },
  {
    id: "cloudSync",
    tab: "advanced",
    section: "cloudSync",
    titleKey: "settings.advanced.cloudSync.title",
    defaultTitle: "Cloud sync",
    keywords: ["webdav", "s3", "sync", "upload", "download", "encryption"],
  },
  {
    id: "connectivityCheck",
    tab: "advanced",
    section: "connectivityCheck",
    titleKey: "settings.advanced.connectivityCheck.title",
    defaultTitle: "Connectivity check",
    keywords: ["stream check", "test", "latency", "timeout"],
  },
  {
    id: "logConfig",
    tab: "advanced",
    section: "logConfig",
    titleKey: "settings.advanced.logConfig.title",
    defaultTitle: "Logging",
    keywords: ["log level", "debug", "logs"],
  },
  // Usage / About
  {
    id: "usage",
    tab: "usage",
    titleKey: "usage.title",
    defaultTitle: "Usage",
    keywords: ["pricing", "cost", "budget", "tokens", "statistics"],
  },
  {
    id: "about",
    tab: "about",
    titleKey: "common.about",
    defaultTitle: "About",
    keywords: ["version", "update", "tools", "install"],
  },
];

type Translate = (key: string, options?: Record<string, unknown>) => string;

/** The entry's title in the UI language. */
export function settingsEntryTitle(
  entry: SettingsSearchEntry,
  t: Translate,
): string {
  const out = t(entry.titleKey, { defaultValue: entry.defaultTitle });
  return out && out !== entry.titleKey ? out : entry.defaultTitle;
}

/**
 * Entries matching every word of `query` (case-insensitive) in the localized
 * title, the English title or the keywords. Title matches rank first.
 */
export function searchSettings(
  query: string,
  t: Translate,
  index: readonly SettingsSearchEntry[] = SETTINGS_SEARCH_INDEX,
): SettingsSearchEntry[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];
  const scored: { entry: SettingsSearchEntry; score: number }[] = [];
  for (const entry of index) {
    const title = settingsEntryTitle(entry, t).toLowerCase();
    const haystack = [
      title,
      entry.defaultTitle.toLowerCase(),
      ...entry.keywords,
    ]
      .join(" \u0000 ")
      .toLowerCase();
    if (!words.every((w) => haystack.includes(w))) continue;
    const titleHit = words.every(
      (w) => title.includes(w) || entry.defaultTitle.toLowerCase().includes(w),
    );
    scored.push({ entry, score: titleHit ? 0 : 1 });
  }
  return scored.sort((a, b) => a.score - b.score).map(({ entry }) => entry);
}
