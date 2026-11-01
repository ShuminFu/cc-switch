import type { Settings } from "@/types";
import { z } from "zod";
import { createStoredValue } from "@/lib/storage";
import { sanitizeDirectoryFields } from "./settingsDirectoryFields";

export type Language = "zh" | "zh-TW" | "en" | "ja";

export type SettingsFormState = Omit<Settings, "language"> & {
  language: Language;
};

export const normalizeLanguage = (lang?: string | null): Language => {
  if (!lang) return "zh";
  const normalized = lang.toLowerCase().replace(/_/g, "-");

  if (normalized === "zh") return "zh";
  if (
    normalized === "zh-tw" ||
    normalized.startsWith("zh-hant") ||
    normalized.startsWith("zh-hk") ||
    normalized.startsWith("zh-mo")
  ) {
    return "zh-TW";
  }
  if (normalized === "en" || normalized === "ja") return normalized;
  return "zh";
};

export const isSupportedLanguage = (lang?: string | null): boolean => {
  if (!lang) return false;
  const normalized = lang.toLowerCase().replace(/_/g, "-");
  return (
    normalized === "en" || normalized === "ja" || normalized.startsWith("zh")
  );
};

/**
 * The persisted UI language (`language` in localStorage). Accepts the same
 * aliases as `isSupportedLanguage` and normalises them (FE-ARCH-22).
 */
export const languagePreference = createStoredValue<Language | undefined>(
  "language",
  z
    .string()
    .refine((value) => isSupportedLanguage(value))
    .transform((value) => normalizeLanguage(value)),
  undefined,
);

/**
 * Form defaults for boolean settings the backend may omit. One entry per
 * setting; both the initial load and a reset go through `normalizeSettingsForm`.
 */
export const SETTINGS_FORM_DEFAULTS = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  useAppWindowControls: false,
  enableClaudePluginIntegration: false,
  silentStartup: false,
  skipClaudeOnboarding: false,
  // Backend default is true for new installs (CFG-38).
  preserveCodexOfficialAuthOnSwitch: true,
  unifyCodexSessionHistory: false,
} satisfies Partial<Settings>;

type DefaultedKey = keyof typeof SETTINGS_FORM_DEFAULTS;

/** Server settings -> form state: fill defaults, trim directory overrides, normalise language. */
export function normalizeSettingsForm(
  data: Settings,
  fallbackLanguage: () => Language,
): SettingsFormState {
  const withDefaults = { ...data } as Settings;
  for (const key of Object.keys(SETTINGS_FORM_DEFAULTS) as DefaultedKey[]) {
    withDefaults[key] = data[key] ?? SETTINGS_FORM_DEFAULTS[key];
  }
  return {
    ...withDefaults,
    ...sanitizeDirectoryFields(data),
    language: normalizeLanguage(data.language ?? fallbackLanguage()),
  };
}

/**
 * Form state -> `Settings` payload for `save_settings`.
 *
 * Cloud sync credentials are saved through their own commands, so they are
 * stripped here. Directory overrides come from `directorySource`: the form
 * itself for an explicit save, the persisted settings for an autosave (the
 * Advanced tab's directory inputs are drafts until the user presses Save).
 */
export function buildSettingsPayload(
  form: SettingsFormState,
  directorySource: Parameters<typeof sanitizeDirectoryFields>[0],
): Settings {
  const {
    webdavSync: _ignoredWebdavSync,
    s3Sync: _ignoredS3Sync,
    ...rest
  } = form;
  return {
    ...rest,
    ...sanitizeDirectoryFields(directorySource),
    language: form.language,
  };
}
