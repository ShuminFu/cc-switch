/**
 * Preset catalog layer (PRODUCT-07).
 *
 * The preset lists in `src/config/*Presets.ts` stay the schema and the
 * offline default. Each list registers itself here; the backend serves a
 * catalog overlay — the bundled (empty) snapshot, or a newer remote catalog
 * whose minisign signature it verified — and the overlay is applied to the
 * registered arrays in place:
 *
 * - `patch[<preset name>]`: replace whitelisted fields (links, endpoint
 *   candidates, partner flags, template config) or `hidden: true` to remove;
 * - `add`: append new data-only presets;
 * - `promotions`: per-locale partner promotion copy.
 *
 * The overlay only changes the templates offered when adding a provider; it
 * never touches providers the user already configured.
 */
import { invoke } from "@tauri-apps/api/core";
import i18n from "i18next";

export type CatalogApp =
  | "claude"
  | "claudeDesktop"
  | "codex"
  | "gemini"
  | "grokbuild"
  | "opencode"
  | "openclaw"
  | "hermes"
  | "universal";

export interface CatalogAppOverlay {
  patch?: Record<string, Record<string, unknown> & { hidden?: boolean }>;
  add?: Array<Record<string, unknown> & { name: string }>;
}

export interface PresetCatalog {
  schema: number;
  version: string;
  publishedAt?: string;
  apps: Partial<Record<CatalogApp, CatalogAppOverlay>>;
  promotions: Record<string, Record<string, string>>;
}

export interface PresetCatalogStatus {
  source: "bundled" | "remote";
  version: string;
  publishedAt?: string;
  fetchedAt?: number;
  lastCheckedAt?: number;
  autoUpdate: boolean;
  lastError?: string;
}

interface CatalogResponse {
  status: PresetCatalogStatus;
  catalog: PresetCatalog;
}

/** Fields a catalog may patch (mirrors the backend whitelist). */
const PATCHABLE_FIELDS = new Set([
  "websiteUrl",
  "apiKeyUrl",
  "endpointCandidates",
  "isPartner",
  "primePartner",
  "partnerPromotionKey",
  "category",
  "icon",
  "iconColor",
  "settingsConfig",
]);

type NamedPreset = { name: string };

const targets = new Map<CatalogApp, NamedPreset[]>();
let appliedVersion: string | null = null;
let loadScheduled = false;
let currentStatus: PresetCatalogStatus | null = null;

/**
 * Apply one app's overlay to `presets` in place. Patched presets keep their
 * object identity. Returns whether anything changed.
 */
export function applyCatalogOverlay<T extends NamedPreset>(
  presets: T[],
  overlay: CatalogAppOverlay | undefined,
): boolean {
  if (!overlay) return false;
  const patch = overlay.patch ?? {};
  const add = overlay.add ?? [];
  if (Object.keys(patch).length === 0 && add.length === 0) return false;

  let changed = false;
  for (let i = presets.length - 1; i >= 0; i--) {
    const fields = patch[presets[i].name];
    if (!fields) continue;
    if (fields.hidden === true) {
      presets.splice(i, 1);
      changed = true;
      continue;
    }
    for (const [key, value] of Object.entries(fields)) {
      if (PATCHABLE_FIELDS.has(key)) {
        (presets[i] as Record<string, unknown>)[key] = value;
        changed = true;
      }
    }
  }
  const existing = new Set(presets.map((p) => p.name));
  for (const item of add) {
    if (typeof item.name !== "string" || existing.has(item.name)) continue;
    if (!item.settingsConfig || typeof item.settingsConfig !== "object")
      continue;
    presets.push(item as unknown as T);
    existing.add(item.name);
    changed = true;
  }
  return changed;
}

/** Register partner promotion copy from the catalog with i18next. */
export function applyCatalogPromotions(
  promotions: PresetCatalog["promotions"] | undefined,
) {
  if (!promotions) return;
  for (const [key, texts] of Object.entries(promotions)) {
    for (const [lang, text] of Object.entries(texts)) {
      if (typeof text !== "string") continue;
      i18n.addResourceBundle?.(
        lang,
        "translation",
        { providerForm: { partnerPromotion: { [key]: text } } },
        true,
        true,
      );
    }
  }
}

/** Apply a catalog to every registered preset list (once per version). */
export function applyCatalog(catalog: PresetCatalog) {
  if (appliedVersion === catalog.version) return;
  appliedVersion = catalog.version;
  for (const [app, presets] of targets) {
    applyCatalogOverlay(presets, catalog.apps?.[app]);
  }
  applyCatalogPromotions(catalog.promotions);
}

async function loadCatalog() {
  try {
    const response = await invoke<CatalogResponse>("get_preset_catalog");
    currentStatus = response.status;
    applyCatalog(response.catalog);
    // Daily (opt-out) check; a newer verified catalog is applied right away.
    const status = await invoke<PresetCatalogStatus>("refresh_preset_catalog");
    currentStatus = status;
    if (status.version !== response.status.version) {
      const updated = await invoke<CatalogResponse>("get_preset_catalog");
      currentStatus = updated.status;
      applyCatalog(updated.catalog);
    }
  } catch (error) {
    // Offline or older backend: the compiled-in presets stay as they are.
    console.warn("[presetCatalog] using bundled presets:", error);
  }
}

function scheduleLoad() {
  if (loadScheduled) return;
  loadScheduled = true;
  // Unit tests apply overlays explicitly; never fetch implicitly there.
  if (import.meta.env?.MODE === "test" || typeof window === "undefined") {
    return;
  }
  setTimeout(() => void loadCatalog(), 0);
}

/**
 * Called by each `*Presets.ts` module with its exported array. Returns the
 * same array so modules can register inline.
 */
export function registerPresetCatalog<T extends NamedPreset>(
  app: CatalogApp,
  presets: T[],
): T[] {
  targets.set(app, presets);
  scheduleLoad();
  return presets;
}

/** Catalog source/version for display ("presets updated X ago"). */
export function getPresetCatalogStatus(): PresetCatalogStatus | null {
  return currentStatus;
}

export function setPresetCatalogAutoUpdate(
  enabled: boolean,
): Promise<PresetCatalogStatus> {
  return invoke<PresetCatalogStatus>("set_preset_catalog_auto_update", {
    enabled,
  }).then((status) => {
    currentStatus = status;
    return status;
  });
}

/** Test helper: forget registrations and the applied version. */
export function resetPresetCatalogForTests() {
  targets.clear();
  appliedVersion = null;
}
