import type { OpenCodeModel, OpenCodeProviderConfig } from "@/types";
import type { PricingModelSourceOption } from "../ProviderAdvancedConfig";
import {
  OPENCODE_DEFAULT_CONFIG,
  OPENCODE_DEFAULT_NPM,
} from "@/config/providerDefaultConfigs";

// Default configs live in src/config/providerDefaultConfigs.ts (FE-ARCH-29).
export {
  CLAUDE_DEFAULT_CONFIG,
  CLAUDE_DESKTOP_DEFAULT_CONFIG,
  CODEX_DEFAULT_CONFIG,
  GEMINI_DEFAULT_CONFIG,
  OPENCLAW_DEFAULT_CONFIG,
  OPENCODE_DEFAULT_CONFIG,
  OPENCODE_DEFAULT_NPM,
} from "@/config/providerDefaultConfigs";

export const OPENCODE_KNOWN_OPTION_KEYS = [
  "baseURL",
  "apiKey",
  "headers",
] as const;

// Contains ":", which is not valid in an HTTP field name, so it cannot
// collide with a legitimate custom header from an existing configuration.
export const OPENCODE_HEADER_DRAFT_PREFIX = "draft-header:";
export const OPENCODE_EXTRA_OPTION_DRAFT_PREFIX = "draft-option:";
/** Unsaved new model rows (no id typed yet); never written to opencode.json. */
export const OPENCODE_MODEL_DRAFT_PREFIX = "draft-model:";
/** Unsaved new per-model option / extra-field rows. */
export const OPENCODE_MODEL_FIELD_DRAFT_PREFIX = "draft-model-field:";

const isDraftKey = (key: string, prefix: string) => key.startsWith(prefix);

/**
 * Drop draft rows before models are written to the config: untouched new
 * model rows and unnamed option / extra-field rows (FE-PROV-20, FE-PROV-28).
 */
export function stripOpencodeModelDrafts(
  models: Record<string, OpenCodeModel>,
): Record<string, OpenCodeModel> {
  const result: Record<string, OpenCodeModel> = {};
  for (const [id, model] of Object.entries(models)) {
    if (isDraftKey(id, OPENCODE_MODEL_DRAFT_PREFIX)) continue;
    const next: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(model)) {
      if (isDraftKey(key, OPENCODE_MODEL_FIELD_DRAFT_PREFIX)) continue;
      if (key === "options" && value && typeof value === "object") {
        const options = Object.fromEntries(
          Object.entries(value as Record<string, unknown>).filter(
            ([optionKey]) =>
              !isDraftKey(optionKey, OPENCODE_MODEL_FIELD_DRAFT_PREFIX),
          ),
        );
        if (Object.keys(options).length > 0) next.options = options;
        continue;
      }
      next[key] = value;
    }
    result[id] = next as OpenCodeModel;
  }
  return result;
}

// ── Pure functions ───────────────────────────────────────────────────

export function isKnownOpencodeOptionKey(key: string): boolean {
  return OPENCODE_KNOWN_OPTION_KEYS.includes(
    key as (typeof OPENCODE_KNOWN_OPTION_KEYS)[number],
  );
}

export function parseOpencodeConfig(
  settingsConfig?: Record<string, unknown>,
): OpenCodeProviderConfig {
  const normalize = (
    parsed: Partial<OpenCodeProviderConfig>,
  ): OpenCodeProviderConfig => ({
    npm: parsed.npm || OPENCODE_DEFAULT_NPM,
    options:
      parsed.options && typeof parsed.options === "object"
        ? (parsed.options as OpenCodeProviderConfig["options"])
        : {},
    models:
      parsed.models && typeof parsed.models === "object"
        ? (parsed.models as Record<string, OpenCodeModel>)
        : {},
  });

  try {
    const parsed = JSON.parse(
      settingsConfig ? JSON.stringify(settingsConfig) : OPENCODE_DEFAULT_CONFIG,
    ) as Partial<OpenCodeProviderConfig>;
    return normalize(parsed);
  } catch {
    return {
      npm: OPENCODE_DEFAULT_NPM,
      options: {},
      models: {},
    };
  }
}

export function parseOpencodeConfigStrict(
  settingsConfig?: Record<string, unknown>,
): OpenCodeProviderConfig {
  const parsed = JSON.parse(
    settingsConfig ? JSON.stringify(settingsConfig) : OPENCODE_DEFAULT_CONFIG,
  ) as Partial<OpenCodeProviderConfig>;
  return {
    npm: parsed.npm || OPENCODE_DEFAULT_NPM,
    options:
      parsed.options && typeof parsed.options === "object"
        ? (parsed.options as OpenCodeProviderConfig["options"])
        : {},
    models:
      parsed.models && typeof parsed.models === "object"
        ? (parsed.models as Record<string, OpenCodeModel>)
        : {},
  };
}

export const OPENCODE_KNOWN_MODEL_KEYS = ["name", "limit", "options"] as const;

export function isKnownModelKey(key: string): boolean {
  return OPENCODE_KNOWN_MODEL_KEYS.includes(
    key as (typeof OPENCODE_KNOWN_MODEL_KEYS)[number],
  );
}

export function getModelExtraFields(
  model: OpenCodeModel,
): Record<string, string> {
  const extra: Record<string, string> = {};
  for (const [k, v] of Object.entries(model)) {
    if (!isKnownModelKey(k)) {
      extra[k] = typeof v === "string" ? v : JSON.stringify(v);
    }
  }
  return extra;
}

export function toOpencodeExtraOptions(
  options: OpenCodeProviderConfig["options"],
): Record<string, string> {
  const extra: Record<string, string> = {};
  for (const [k, v] of Object.entries(options || {})) {
    if (!isKnownOpencodeOptionKey(k)) {
      extra[k] = typeof v === "string" ? v : JSON.stringify(v);
    }
  }
  return extra;
}

export { buildOmoProfilePreview } from "@/config/omo";

export const normalizePricingSource = (
  value?: string,
): PricingModelSourceOption =>
  value === "request" || value === "response" ? value : "inherit";
