import type { AppId } from "@/lib/api/types";
import type { ProviderCategory } from "@/types";
import {
  providerPresets,
  type ProviderPreset,
} from "@/config/claudeProviderPresets";
import {
  codexProviderPresets,
  type CodexProviderPreset,
} from "@/config/codexProviderPresets";
import {
  geminiProviderPresets,
  type GeminiProviderPreset,
} from "@/config/geminiProviderPresets";
import {
  opencodeProviderPresets,
  type OpenCodeProviderPreset,
} from "@/config/opencodeProviderPresets";
import {
  openclawProviderPresets,
  type OpenClawProviderPreset,
} from "@/config/openclawProviderPresets";
import {
  hermesProviderPresets,
  type HermesProviderPreset,
} from "@/config/hermesProviderPresets";
import {
  claudeDesktopProviderPresets,
  type ClaudeDesktopProviderPreset,
} from "@/config/claudeDesktopProviderPresets";

/**
 * Stable preset identities (FE-PROV-19).
 *
 * Preset ids used to be array indices (`codex-3`), re-parsed with regexes in
 * half a dozen places, so inserting or reordering a preset silently changed
 * which preset an id referred to (and the Claude list was filtered for hidden
 * presets before indexing, so ids and indices already disagreed). Every
 * consumer now asks this registry instead: ids are `<app>:<slug>` where the
 * slug is derived from the preset's own `id` (when it has one) or its name,
 * which survives reordering.
 */

export interface PresetTypeMap {
  claude: ProviderPreset;
  "claude-desktop": ClaudeDesktopProviderPreset;
  codex: CodexProviderPreset;
  gemini: GeminiProviderPreset;
  opencode: OpenCodeProviderPreset;
  openclaw: OpenClawProviderPreset;
  hermes: HermesProviderPreset;
  /** Grok Build reuses the non-official Codex presets. */
  grokbuild: CodexProviderPreset;
}

export type AnyProviderPreset = PresetTypeMap[keyof PresetTypeMap];

export interface PresetEntry<P = AnyProviderPreset> {
  id: string;
  preset: P;
}

/** Sentinel id of the "custom" (no preset) choice in the preset selector. */
export const CUSTOM_PRESET_ID = "custom";

/** Lowercase, keep letters/digits of any script, collapse the rest to `-`. */
export const slugifyPresetName = (name: string): string =>
  name
    .normalize("NFKC")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, "-")
    .replace(/^-+|-+$/g, "");

const SOURCES: { [K in AppId]: () => readonly PresetTypeMap[K][] } = {
  claude: () => providerPresets.filter((preset) => !preset.hidden),
  "claude-desktop": () => claudeDesktopProviderPresets,
  codex: () => codexProviderPresets,
  gemini: () => geminiProviderPresets,
  opencode: () => opencodeProviderPresets,
  openclaw: () => openclawProviderPresets,
  hermes: () => hermesProviderPresets,
  grokbuild: () =>
    codexProviderPresets.filter(
      (preset) => preset.category !== "official" && !preset.isOfficial,
    ),
};

/**
 * Build entries with unique ids. A duplicate slug inside one app gets a
 * numeric suffix (`-2`, `-3`, ...) in list order, so only true duplicates
 * depend on position.
 */
export function buildPresetEntries<P extends { name: string; id?: string }>(
  appId: AppId,
  presets: readonly P[],
): PresetEntry<P>[] {
  const seen = new Map<string, number>();
  return presets.map((preset) => {
    const base =
      (typeof preset.id === "string" && preset.id.trim()) ||
      slugifyPresetName(preset.name) ||
      "preset";
    const count = (seen.get(base) ?? 0) + 1;
    seen.set(base, count);
    const slug = count === 1 ? base : `${base}-${count}`;
    return { id: `${appId}:${slug}`, preset };
  });
}

const cache = new Map<AppId, PresetEntry[]>();

/** All selectable presets of an app, in display order, with stable ids. */
export function getPresetEntries<K extends AppId>(
  appId: K,
): PresetEntry<PresetTypeMap[K]>[] {
  let entries = cache.get(appId);
  if (!entries) {
    entries = buildPresetEntries(
      appId,
      SOURCES[appId]() as readonly (AnyProviderPreset & { id?: string })[],
    );
    cache.set(appId, entries);
  }
  return entries as PresetEntry<PresetTypeMap[K]>[];
}

/** Look a preset up by id; `undefined` for `custom`, unknown or foreign ids. */
export function findPresetEntry<K extends AppId>(
  appId: K,
  presetId: string | null | undefined,
): PresetEntry<PresetTypeMap[K]> | undefined {
  if (!presetId || presetId === CUSTOM_PRESET_ID) return undefined;
  return getPresetEntries(appId).find((entry) => entry.id === presetId);
}

/** Category a preset assigns to new providers. */
export function getPresetCategory(
  preset: AnyProviderPreset,
): ProviderCategory | undefined {
  if (preset.category) return preset.category;
  return "isOfficial" in preset && preset.isOfficial ? "official" : undefined;
}

/**
 * A preset's config in the shape of a stored provider's `settingsConfig`, so
 * provider codecs can read presets too (Codex/Grok Build presets keep the
 * TOML in `config`, Claude Desktop presets carry a top-level `baseUrl`).
 */
export function presetSettingsConfig(preset: AnyProviderPreset): unknown {
  const candidate = preset as {
    settingsConfig?: unknown;
    auth?: unknown;
    config?: unknown;
    baseUrl?: unknown;
  };
  if (candidate.settingsConfig !== undefined) return candidate.settingsConfig;
  if (typeof candidate.config === "string") {
    return { auth: candidate.auth ?? {}, config: candidate.config };
  }
  if (typeof candidate.baseUrl === "string") {
    return { env: { ANTHROPIC_BASE_URL: candidate.baseUrl } };
  }
  return undefined;
}
