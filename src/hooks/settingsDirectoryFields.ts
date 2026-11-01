import type { AppId } from "@/lib/api";
import type { Settings } from "@/types";

/** Apps whose config directory can be overridden in Settings → Advanced. */
export type DirectoryAppId = Exclude<AppId, "claude-desktop">;

/**
 * Every per-app config directory override field on `Settings`.
 *
 * This is the single list that payload building, form normalisation and
 * directory-change detection iterate over, so adding a new app only needs
 * one entry here (the type check below fails if a `*ConfigDir` field is
 * missing).
 */
export type DirectorySettingsField = {
  [K in keyof Settings]-?: K extends `${string}ConfigDir` ? K : never;
}[keyof Settings];

/**
 * Single source of truth for per-app directory metadata: the settings field
 * that stores the override and the default folder under the home directory.
 * Key order is the order the directory inputs are rendered in.
 */
export const APP_DIRECTORY_META: Record<
  DirectoryAppId,
  { field: DirectorySettingsField; defaultFolder: string }
> = {
  claude: { field: "claudeConfigDir", defaultFolder: ".claude" },
  codex: { field: "codexConfigDir", defaultFolder: ".codex" },
  gemini: { field: "geminiConfigDir", defaultFolder: ".gemini" },
  grokbuild: { field: "grokConfigDir", defaultFolder: ".grok" },
  opencode: { field: "opencodeConfigDir", defaultFolder: ".config/opencode" },
  openclaw: { field: "openclawConfigDir", defaultFolder: ".openclaw" },
  hermes: { field: "hermesConfigDir", defaultFolder: ".hermes" },
};

export const DIRECTORY_APP_IDS = Object.keys(
  APP_DIRECTORY_META,
) as DirectoryAppId[];

// Exhaustiveness check: every `*ConfigDir` field must belong to an app above.
const DIRECTORY_FIELD_MAP: Record<DirectorySettingsField, DirectoryAppId> = {
  claudeConfigDir: "claude",
  codexConfigDir: "codex",
  geminiConfigDir: "gemini",
  grokConfigDir: "grokbuild",
  opencodeConfigDir: "opencode",
  openclawConfigDir: "openclaw",
  hermesConfigDir: "hermes",
};

export const DIRECTORY_SETTINGS_FIELDS = Object.keys(
  DIRECTORY_FIELD_MAP,
) as DirectorySettingsField[];

export const sanitizeDir = (value?: string | null): string | undefined => {
  if (!value) return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
};

type DirectorySource =
  | Partial<Pick<Settings, DirectorySettingsField>>
  | null
  | undefined;

/** Returns a copy of every directory override field, trimmed (empty -> undefined). */
export const sanitizeDirectoryFields = (
  source: DirectorySource,
): Pick<Settings, DirectorySettingsField> => {
  const result = {} as Pick<Settings, DirectorySettingsField>;
  for (const field of DIRECTORY_SETTINGS_FIELDS) {
    result[field] = sanitizeDir(source?.[field]);
  }
  return result;
};

/** Per-app view of the directory override fields (raw unless `transform` is given). */
export const directoryValuesByApp = (
  source: DirectorySource,
  transform: (value?: string | null) => string | undefined = (value) =>
    value ?? undefined,
): Record<DirectoryAppId, string | undefined> => {
  const result = {} as Record<DirectoryAppId, string | undefined>;
  for (const app of DIRECTORY_APP_IDS) {
    result[app] = transform(source?.[APP_DIRECTORY_META[app].field]);
  }
  return result;
};

/** Directory fields whose sanitised value differs between `next` and `prev`. */
export const changedDirectoryFields = (
  next: DirectorySource,
  prev: DirectorySource,
): DirectorySettingsField[] =>
  DIRECTORY_SETTINGS_FIELDS.filter(
    (field) => sanitizeDir(next?.[field]) !== sanitizeDir(prev?.[field]),
  );
