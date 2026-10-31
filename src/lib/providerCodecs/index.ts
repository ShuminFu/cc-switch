import type { AppId } from "@/lib/api/types";
import type { Provider } from "@/types";
import {
  extractCodexBaseUrl,
  extractCodexExperimentalBearerToken,
  extractCodexModelName,
} from "@/utils/providerConfigUtils";
import { parseGrokBuildConfig } from "@/utils/grokBuildConfig";
import {
  CLAUDE_DEFAULT_CONFIG,
  CLAUDE_DESKTOP_DEFAULT_CONFIG,
  CODEX_DEFAULT_CONFIG,
  GEMINI_DEFAULT_CONFIG,
  OPENCLAW_DEFAULT_CONFIG,
  OPENCODE_DEFAULT_CONFIG,
} from "@/components/providers/forms/helpers/opencodeFormUtils";

/**
 * Per-app knowledge of a provider's `settingsConfig` shape (FE-PROV-22).
 *
 * Cards, dialogs, the speed-test seeding and the provider form adapters used
 * to re-derive the same facts (endpoint, API key, models, "is official") from
 * raw JSON/TOML in several places, and the shared helpers drifted (the Codex
 * API key was read from a non-existent `env.CODEX_API_KEY`). Every consumer
 * now goes through one codec per app.
 *
 * Shapes (see also docs in each codec):
 * - claude / claude-desktop: `{ env: { ANTHROPIC_BASE_URL, ANTHROPIC_AUTH_TOKEN | ANTHROPIC_API_KEY, ANTHROPIC_MODEL, ... } }`
 *   (Bedrock-style presets may carry a top-level `apiKey`)
 * - codex: `{ auth: { OPENAI_API_KEY }, config: "<config.toml>", modelCatalog? }`
 * - gemini: `{ env: { GOOGLE_GEMINI_BASE_URL, GEMINI_API_KEY, GEMINI_MODEL }, config: {} }`
 * - grokbuild: `{ config: "<config.toml>" }` with `[model.<profile>] base_url / api_key`
 * - opencode: `{ npm, options: { baseURL, apiKey, ... }, models: { [id]: {...} } }`
 * - openclaw: `{ baseUrl, apiKey, api, models: [{ id, ... }] }`
 * - hermes: `{ name, base_url, api_key, api_mode?, models?: [{ id }] }`
 */
export interface ProviderCodec {
  readonly appId: AppId;
  /** settingsConfig JSON text for a brand-new custom provider. */
  readonly defaultConfig: string;
  /** Upstream endpoint (base URL), or `undefined` when none is configured. */
  getEndpoint(config: unknown): string | undefined;
  /**
   * API key, or `""` when none is configured. `apiKeyField` (from
   * `meta.apiKeyField`) pins which env variable holds the key.
   */
  getApiKey(config: unknown, options?: { apiKeyField?: string }): string;
  /** Whether the config has a slot for an API key (even an empty one). */
  hasApiKeyField(config: unknown): boolean;
  /**
   * Return a copy of `config` with the API key written. Without
   * `createIfMissing` a config that has no key slot is returned unchanged.
   */
  setApiKey(
    config: Record<string, unknown>,
    apiKey: string,
    options?: { createIfMissing?: boolean; apiKeyField?: string },
  ): Record<string, unknown>;
  /** Configured model ids, primary first, without duplicates. */
  getModels(config: unknown): string[];
  /** Whether the provider talks to the vendor's own API with its own login. */
  isOfficial(config: unknown): boolean;
}

type Rec = Record<string, unknown>;

const asRecord = (value: unknown): Rec | undefined =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Rec)
    : undefined;

const nonEmpty = (value: unknown): string | undefined =>
  typeof value === "string" && value.trim() ? value.trim() : undefined;

const str = (value: unknown): string =>
  typeof value === "string" ? value : "";

const has = (obj: Rec | undefined, key: string) =>
  !!obj && Object.prototype.hasOwnProperty.call(obj, key);

const unique = (values: Array<string | undefined>): string[] => {
  const out: string[] = [];
  for (const value of values) {
    const trimmed = value?.trim();
    if (trimmed && !out.includes(trimmed)) out.push(trimmed);
  }
  return out;
};

const clone = (config: Rec): Rec => JSON.parse(JSON.stringify(config)) as Rec;

const CLAUDE_MODEL_KEYS = [
  "ANTHROPIC_MODEL",
  "ANTHROPIC_DEFAULT_SONNET_MODEL",
  "ANTHROPIC_DEFAULT_OPUS_MODEL",
  "ANTHROPIC_DEFAULT_HAIKU_MODEL",
  "ANTHROPIC_SMALL_FAST_MODEL",
] as const;

const CLAUDE_KEY_FIELDS = ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"];

/**
 * Which env variable holds a Claude API key (FE-ARCH-47): an explicit
 * `meta.apiKeyField` first, then whichever of AUTH_TOKEN / API_KEY is
 * non-empty, then whichever exists. An empty `ANTHROPIC_AUTH_TOKEN` next to a
 * populated `ANTHROPIC_API_KEY` must not hide the real key.
 */
export const resolveClaudeApiKeyField = (
  env: Rec | undefined,
  apiKeyField?: string,
): string | undefined => {
  if (apiKeyField && CLAUDE_KEY_FIELDS.includes(apiKeyField)) {
    return apiKeyField;
  }
  return (
    CLAUDE_KEY_FIELDS.find((key) => nonEmpty(env?.[key])) ??
    CLAUDE_KEY_FIELDS.find((key) => has(env, key))
  );
};

const makeClaudeCodec = (
  appId: "claude" | "claude-desktop",
  defaultConfig: string,
): ProviderCodec => ({
  appId,
  defaultConfig,
  getEndpoint: (config) =>
    nonEmpty(asRecord(asRecord(config)?.env)?.ANTHROPIC_BASE_URL),
  getApiKey: (config, options = {}) => {
    const root = asRecord(config);
    // Top-level apiKey is used by Bedrock-style presets; ignore templates.
    const topLevel = root?.apiKey;
    if (typeof topLevel === "string" && topLevel && !topLevel.includes("${")) {
      return topLevel;
    }
    const env = asRecord(root?.env);
    const field = resolveClaudeApiKeyField(env, options.apiKeyField);
    return field ? str(env?.[field]) : "";
  },
  hasApiKeyField: (config) => {
    const root = asRecord(config);
    const env = asRecord(root?.env);
    return (
      has(root, "apiKey") ||
      has(env, "ANTHROPIC_AUTH_TOKEN") ||
      has(env, "ANTHROPIC_API_KEY")
    );
  },
  setApiKey: (config, apiKey, options = {}) => {
    const next = clone(config);
    if (has(next, "apiKey")) {
      next.apiKey = apiKey;
      return next;
    }
    let env = asRecord(next.env);
    if (!env) {
      if (!options.createIfMissing) return config;
      env = {};
      next.env = env;
    }
    const pinned =
      options.apiKeyField && CLAUDE_KEY_FIELDS.includes(options.apiKeyField)
        ? options.apiKeyField
        : undefined;
    const existing = resolveClaudeApiKeyField(env);
    let field: string | undefined;
    if (pinned && (has(env, pinned) || options.createIfMissing)) field = pinned;
    else if (existing) field = existing;
    else if (options.createIfMissing) field = pinned ?? "ANTHROPIC_AUTH_TOKEN";
    if (!field) return config;
    env[field] = apiKey;
    return next;
  },
  getModels: (config) => {
    const env = asRecord(asRecord(config)?.env);
    return unique(CLAUDE_MODEL_KEYS.map((key) => nonEmpty(env?.[key])));
  },
  isOfficial: (config) =>
    !nonEmpty(asRecord(asRecord(config)?.env)?.ANTHROPIC_BASE_URL),
});

const codexCodec: ProviderCodec = {
  appId: "codex",
  defaultConfig: CODEX_DEFAULT_CONFIG,
  getEndpoint: (config) =>
    nonEmpty(extractCodexBaseUrl(str(asRecord(config)?.config))),
  getApiKey: (config) => str(asRecord(asRecord(config)?.auth)?.OPENAI_API_KEY),
  hasApiKeyField: (config) =>
    has(asRecord(asRecord(config)?.auth), "OPENAI_API_KEY"),
  setApiKey: (config, apiKey, options = {}) => {
    const next = clone(config);
    let auth = asRecord(next.auth);
    if (!auth) {
      if (!options.createIfMissing) return config;
      auth = {};
      next.auth = auth;
    }
    if (!has(auth, "OPENAI_API_KEY") && !options.createIfMissing) {
      return config;
    }
    auth.OPENAI_API_KEY = apiKey;
    return next;
  },
  getModels: (config) => {
    const root = asRecord(config);
    const catalog = asRecord(root?.modelCatalog)?.models;
    return unique([
      extractCodexModelName(str(root?.config)),
      ...(Array.isArray(catalog)
        ? catalog.map((item) => nonEmpty(asRecord(item)?.model))
        : []),
    ]);
  },
  isOfficial: (config) => {
    const root = asRecord(config);
    // No OPENAI_API_KEY and no bearer token: Codex CLI's own ChatGPT login.
    return (
      !nonEmpty(asRecord(root?.auth)?.OPENAI_API_KEY) &&
      !extractCodexExperimentalBearerToken(str(root?.config))
    );
  },
};

const geminiCodec: ProviderCodec = {
  appId: "gemini",
  defaultConfig: GEMINI_DEFAULT_CONFIG,
  getEndpoint: (config) =>
    nonEmpty(asRecord(asRecord(config)?.env)?.GOOGLE_GEMINI_BASE_URL),
  getApiKey: (config) => str(asRecord(asRecord(config)?.env)?.GEMINI_API_KEY),
  hasApiKeyField: (config) =>
    has(asRecord(asRecord(config)?.env), "GEMINI_API_KEY"),
  setApiKey: (config, apiKey, options = {}) => {
    const next = clone(config);
    let env = asRecord(next.env);
    if (!env) {
      if (!options.createIfMissing) return config;
      env = {};
      next.env = env;
    }
    if (!has(env, "GEMINI_API_KEY") && !options.createIfMissing) return config;
    env.GEMINI_API_KEY = apiKey;
    return next;
  },
  getModels: (config) =>
    unique([nonEmpty(asRecord(asRecord(config)?.env)?.GEMINI_MODEL)]),
  isOfficial: (config) => {
    // No key and no base URL: Google OAuth login.
    const env = asRecord(asRecord(config)?.env);
    return (
      !nonEmpty(env?.GEMINI_API_KEY) && !nonEmpty(env?.GOOGLE_GEMINI_BASE_URL)
    );
  },
};

const grokParse = (config: unknown) => {
  const toml = str(asRecord(config)?.config);
  return toml.trim() ? parseGrokBuildConfig(toml) : undefined;
};

const grokbuildCodec: ProviderCodec = {
  appId: "grokbuild",
  defaultConfig: JSON.stringify({ config: "" }, null, 2),
  getEndpoint: (config) => nonEmpty(grokParse(config)?.baseUrl),
  getApiKey: (config) => grokParse(config)?.apiKey ?? "",
  hasApiKeyField: (config) => !!grokParse(config),
  // The TOML is edited by the Grok Build form itself.
  setApiKey: (config) => config,
  getModels: (config) => {
    const parsed = grokParse(config);
    return parsed ? unique([parsed.upstreamModel, parsed.model]) : [];
  },
  isOfficial: () => false,
};

/** Codec for apps whose config is a flat object with plain key/url fields. */
const makeFlatCodec = (
  appId: AppId,
  defaultConfig: string,
  fields: { endpoint: string[]; apiKey: string[] },
  getModels: (root: Rec | undefined) => Array<string | undefined>,
): ProviderCodec => {
  const container = (root: Rec | undefined, path: string[]) =>
    path
      .slice(0, -1)
      .reduce<Rec | undefined>((acc, key) => asRecord(acc?.[key]), root);
  const leaf = (path: string[]) => path[path.length - 1];
  return {
    appId,
    defaultConfig,
    getEndpoint: (config) =>
      nonEmpty(
        container(asRecord(config), fields.endpoint)?.[leaf(fields.endpoint)],
      ),
    getApiKey: (config) =>
      str(container(asRecord(config), fields.apiKey)?.[leaf(fields.apiKey)]),
    hasApiKeyField: (config) =>
      has(container(asRecord(config), fields.apiKey), leaf(fields.apiKey)),
    setApiKey: (config, apiKey, options = {}) => {
      const next = clone(config);
      let target: Rec = next;
      for (const key of fields.apiKey.slice(0, -1)) {
        let child = asRecord(target[key]);
        if (!child) {
          if (!options.createIfMissing) return config;
          child = {};
          target[key] = child;
        }
        target = child;
      }
      const key = leaf(fields.apiKey);
      if (!has(target, key) && !options.createIfMissing) return config;
      target[key] = apiKey;
      return next;
    },
    getModels: (config) => unique(getModels(asRecord(config))),
    isOfficial: () => false,
  };
};

const modelIds = (models: unknown): Array<string | undefined> =>
  Array.isArray(models) ? models.map((m) => nonEmpty(asRecord(m)?.id)) : [];

export const HERMES_DEFAULT_CONFIG = JSON.stringify(
  { name: "", base_url: "", api_key: "" },
  null,
  2,
);

const CODECS: Readonly<Record<AppId, ProviderCodec>> = {
  claude: makeClaudeCodec("claude", CLAUDE_DEFAULT_CONFIG),
  "claude-desktop": makeClaudeCodec(
    "claude-desktop",
    CLAUDE_DESKTOP_DEFAULT_CONFIG,
  ),
  codex: codexCodec,
  gemini: geminiCodec,
  grokbuild: grokbuildCodec,
  opencode: makeFlatCodec(
    "opencode",
    OPENCODE_DEFAULT_CONFIG,
    { endpoint: ["options", "baseURL"], apiKey: ["options", "apiKey"] },
    (root) => Object.keys(asRecord(root?.models) ?? {}),
  ),
  openclaw: makeFlatCodec(
    "openclaw",
    OPENCLAW_DEFAULT_CONFIG,
    { endpoint: ["baseUrl"], apiKey: ["apiKey"] },
    (root) => modelIds(root?.models),
  ),
  hermes: makeFlatCodec(
    "hermes",
    HERMES_DEFAULT_CONFIG,
    { endpoint: ["base_url"], apiKey: ["api_key"] },
    (root) => modelIds(root?.models),
  ),
};

export const getProviderCodec = (appId: AppId): ProviderCodec =>
  CODECS[appId] ?? CODECS.claude;

/** Upstream endpoint of a stored provider (FE-PROV-09). */
export const getProviderEndpoint = (
  appId: AppId,
  provider: Pick<Provider, "settingsConfig">,
): string | undefined =>
  getProviderCodec(appId).getEndpoint(provider.settingsConfig);

/** Configured models of a stored provider, primary first. */
export const getProviderModels = (
  appId: AppId,
  provider: Pick<Provider, "settingsConfig">,
): string[] => getProviderCodec(appId).getModels(provider.settingsConfig);

/** Parse a settingsConfig JSON string; `undefined` when it is not an object. */
export const parseSettingsConfig = (json: string): Rec | undefined => {
  try {
    return asRecord(JSON.parse(json));
  } catch {
    return undefined;
  }
};
