import type { Provider } from "@/types";
import {
  extractCodexBaseUrl,
  extractCodexModelName,
} from "@/utils/providerConfigUtils";
import { parseGrokBuildConfig } from "@/utils/grokBuildConfig";

/**
 * Apps whose providers can be turned back into a `ccswitch://` import link.
 * The deep-link parser accepts more app ids, but these are the ones whose
 * stored config shape is known well enough to derive parameters and redact
 * credentials reliably.
 */
export const SHAREABLE_APPS = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
] as const;
export type ShareableAppId = (typeof SHAREABLE_APPS)[number];

export const isShareableApp = (appId: string): appId is ShareableAppId =>
  (SHAREABLE_APPS as ReadonlyArray<string>).includes(appId);

export interface ProviderDeepLinkOptions {
  /** Embed the API key in the link (and keep it inside the attached config). */
  includeApiKey: boolean;
}

const asRecord = (value: unknown): Record<string, unknown> | undefined =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;

const asString = (value: unknown): string | undefined =>
  typeof value === "string" && value.trim() ? value.trim() : undefined;

/** Base64 of the UTF-8 bytes of `text` (matches the backend's decoder). */
export const encodeDeepLinkConfig = (text: string): string => {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
};

const CODEX_BEARER_TOKEN_LINE = /^[ \t]*experimental_bearer_token[ \t]*=.*$/gm;
const GROK_API_KEY_LINE = /^[ \t]*api_key[ \t]*=.*$/gm;

const stripTomlLines = (text: string, pattern: RegExp): string =>
  text
    .replace(pattern, "")
    .replace(/\n{3,}/g, "\n\n")
    .trimEnd()
    .concat("\n");

const extractCodexBearerToken = (text: string): string | undefined => {
  const match = text.match(
    /^[ \t]*experimental_bearer_token[ \t]*=[ \t]*(["'])([^"'\r\n]+)\1/m,
  );
  return match?.[2]?.trim() || undefined;
};

interface DerivedProviderParams {
  endpoint?: string;
  apiKey?: string;
  model?: string;
  haikuModel?: string;
  sonnetModel?: string;
  opusModel?: string;
  /** settingsConfig with credentials removed (when not sharing the key) */
  config: Record<string, unknown>;
}

const CLAUDE_KEY_FIELDS = ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"];
const GEMINI_KEY_FIELDS = ["GEMINI_API_KEY", "GOOGLE_API_KEY"];

const deriveParams = (
  provider: Provider,
  appId: ShareableAppId,
  includeApiKey: boolean,
): DerivedProviderParams => {
  const config = JSON.parse(
    JSON.stringify(provider.settingsConfig ?? {}),
  ) as Record<string, unknown>;

  switch (appId) {
    case "claude": {
      const env = asRecord(config.env) ?? {};
      const apiKey =
        asString(env.ANTHROPIC_AUTH_TOKEN) ?? asString(env.ANTHROPIC_API_KEY);
      if (!includeApiKey) {
        for (const field of CLAUDE_KEY_FIELDS) delete env[field];
      }
      return {
        endpoint: asString(env.ANTHROPIC_BASE_URL),
        apiKey,
        model: asString(env.ANTHROPIC_MODEL),
        haikuModel: asString(env.ANTHROPIC_DEFAULT_HAIKU_MODEL),
        sonnetModel: asString(env.ANTHROPIC_DEFAULT_SONNET_MODEL),
        opusModel: asString(env.ANTHROPIC_DEFAULT_OPUS_MODEL),
        config,
      };
    }
    case "gemini": {
      const env = asRecord(config.env) ?? {};
      const apiKey =
        asString(env.GEMINI_API_KEY) ?? asString(env.GOOGLE_API_KEY);
      if (!includeApiKey) {
        for (const field of GEMINI_KEY_FIELDS) delete env[field];
      }
      // The importer reads Gemini keys from the top level of the payload
      // (merge_gemini_config), matching the flat shape the preview expects.
      return {
        endpoint: asString(env.GOOGLE_GEMINI_BASE_URL),
        apiKey,
        model: asString(env.GEMINI_MODEL),
        config: env,
      };
    }
    case "codex": {
      const auth = asRecord(config.auth) ?? {};
      const tomlText = typeof config.config === "string" ? config.config : "";
      const apiKey =
        asString(auth.OPENAI_API_KEY) ?? extractCodexBearerToken(tomlText);
      if (!includeApiKey) {
        delete auth.OPENAI_API_KEY;
        if (tomlText) {
          config.config = stripTomlLines(tomlText, CODEX_BEARER_TOKEN_LINE);
        }
      }
      return {
        endpoint: extractCodexBaseUrl(tomlText),
        apiKey,
        model: extractCodexModelName(tomlText),
        config,
      };
    }
    case "grokbuild": {
      const tomlText = typeof config.config === "string" ? config.config : "";
      const parsed = parseGrokBuildConfig(tomlText, provider.name);
      if (!includeApiKey && tomlText) {
        config.config = stripTomlLines(tomlText, GROK_API_KEY_LINE);
      }
      return {
        endpoint: asString(parsed.baseUrl),
        apiKey: asString(parsed.apiKey),
        model: asString(parsed.upstreamModel) ?? asString(parsed.model),
        config,
      };
    }
  }
};

const collectEndpoints = (
  provider: Provider,
  primary: string | undefined,
): string | undefined => {
  const urls: string[] = [];
  const push = (raw?: string) => {
    const url = raw?.trim().replace(/\/+$/, "");
    if (url && /^https?:\/\//i.test(url) && !urls.includes(url)) urls.push(url);
  };
  push(primary);
  for (const key of Object.keys(provider.meta?.custom_endpoints ?? {})) {
    push(key);
  }
  return urls.length ? urls.join(",") : undefined;
};

/**
 * Build a `ccswitch://v1/import` link that recreates `provider` on another
 * machine. Explicit parameters are emitted for the common fields and the full
 * settings config travels as the Base64 `config` payload (credentials
 * stripped unless `includeApiKey`), so custom env / TOML extras survive.
 */
export const buildProviderDeepLink = (
  provider: Provider,
  appId: ShareableAppId,
  options: ProviderDeepLinkOptions,
): string => {
  const derived = deriveParams(provider, appId, options.includeApiKey);
  const params = new URLSearchParams();
  params.set("resource", "provider");
  params.set("app", appId);
  params.set("name", provider.name);

  const endpoint = collectEndpoints(provider, derived.endpoint);
  if (endpoint) params.set("endpoint", endpoint);
  if (options.includeApiKey && derived.apiKey) {
    params.set("apiKey", derived.apiKey);
  }
  if (derived.model) params.set("model", derived.model);
  if (derived.haikuModel) params.set("haikuModel", derived.haikuModel);
  if (derived.sonnetModel) params.set("sonnetModel", derived.sonnetModel);
  if (derived.opusModel) params.set("opusModel", derived.opusModel);

  const homepage = asString(provider.websiteUrl);
  if (homepage) params.set("homepage", homepage);
  const icon = asString(provider.icon);
  if (icon) params.set("icon", icon);
  const notes = asString(provider.notes);
  if (notes) params.set("notes", notes);

  params.set("config", encodeDeepLinkConfig(JSON.stringify(derived.config)));
  params.set("configFormat", "json");

  return `ccswitch://v1/import?${params.toString()}`;
};
