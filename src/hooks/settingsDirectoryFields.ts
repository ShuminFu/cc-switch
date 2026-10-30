import type { Settings } from "@/types";

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

const DIRECTORY_FIELD_MAP: Record<DirectorySettingsField, true> = {
  claudeConfigDir: true,
  codexConfigDir: true,
  geminiConfigDir: true,
  grokConfigDir: true,
  opencodeConfigDir: true,
  openclawConfigDir: true,
  hermesConfigDir: true,
};

export const DIRECTORY_SETTINGS_FIELDS = Object.keys(
  DIRECTORY_FIELD_MAP,
) as DirectorySettingsField[];

export const sanitizeDir = (value?: string | null): string | undefined => {
  if (!value) return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
};

/** Returns a copy of every directory override field, trimmed (empty -> undefined). */
export const sanitizeDirectoryFields = (
  source: Partial<Pick<Settings, DirectorySettingsField>> | null | undefined,
): Pick<Settings, DirectorySettingsField> => {
  const result = {} as Pick<Settings, DirectorySettingsField>;
  for (const field of DIRECTORY_SETTINGS_FIELDS) {
    result[field] = sanitizeDir(source?.[field]);
  }
  return result;
};

/** Directory fields whose sanitised value differs between `next` and `prev`. */
export const changedDirectoryFields = (
  next: Partial<Pick<Settings, DirectorySettingsField>> | null | undefined,
  prev: Partial<Pick<Settings, DirectorySettingsField>> | null | undefined,
): DirectorySettingsField[] =>
  DIRECTORY_SETTINGS_FIELDS.filter(
    (field) => sanitizeDir(next?.[field]) !== sanitizeDir(prev?.[field]),
  );
