import type { AppId } from "@/lib/api/types";
import type { Provider } from "@/types";
import { getProviderCodec } from "@/lib/providerCodecs";

/**
 * Build a `ccswitch://v1/import` link that re-creates `provider` through the
 * existing importer (FE-PROV-55). Parameter names mirror
 * `src-tauri/src/deeplink/parser.rs::parse_provider_deeplink`.
 *
 * The API key is left out unless `includeApiKey` is set; callers must ask the
 * user before including it.
 */
export interface BuildProviderDeeplinkOptions {
  includeApiKey?: boolean;
}

const asRecord = (value: unknown): Record<string, unknown> | undefined =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;

const nonEmpty = (value: unknown): string | undefined =>
  typeof value === "string" && value.trim() ? value.trim() : undefined;

export function buildProviderDeeplink(
  provider: Provider,
  appId: AppId,
  options: BuildProviderDeeplinkOptions = {},
): string {
  const codec = getProviderCodec(appId);
  const config = provider.settingsConfig;
  const params = new URLSearchParams();
  params.set("resource", "provider");
  params.set("app", appId);
  params.set("name", provider.name);

  const endpoint = codec.getEndpoint(config);
  if (endpoint) params.set("endpoint", endpoint);
  const homepage = nonEmpty(provider.websiteUrl);
  if (homepage) params.set("homepage", homepage);
  if (options.includeApiKey) {
    const apiKey = codec.getApiKey(config, {
      apiKeyField: provider.meta?.apiKeyField,
    });
    if (apiKey) params.set("apiKey", apiKey);
  }

  if (appId === "claude" || appId === "claude-desktop") {
    const env = asRecord(asRecord(config)?.env);
    const model = nonEmpty(env?.ANTHROPIC_MODEL);
    const haiku = nonEmpty(env?.ANTHROPIC_DEFAULT_HAIKU_MODEL);
    const sonnet = nonEmpty(env?.ANTHROPIC_DEFAULT_SONNET_MODEL);
    const opus = nonEmpty(env?.ANTHROPIC_DEFAULT_OPUS_MODEL);
    if (model) params.set("model", model);
    if (haiku) params.set("haikuModel", haiku);
    if (sonnet) params.set("sonnetModel", sonnet);
    if (opus) params.set("opusModel", opus);
  } else {
    const [model] = codec.getModels(config);
    if (model) params.set("model", model);
  }

  const notes = nonEmpty(provider.notes);
  if (notes) params.set("notes", notes);
  const icon = nonEmpty(provider.icon);
  if (icon) params.set("icon", icon.toLowerCase());

  return `ccswitch://v1/import?${params.toString()}`;
}
