/**
 * Marker field and source values that `apps/hermes/config.rs::get_providers`
 * injects onto each settings payload. Kept in sync with the Rust constants
 * `PROVIDER_SOURCE_FIELD` / `PROVIDER_SOURCE_CUSTOM_LIST` / `PROVIDER_SOURCE_DICT`.
 */
export const HERMES_PROVIDER_SOURCE_FIELD = "_cc_source";
export const HERMES_PROVIDER_SOURCE_CUSTOM_LIST = "custom_providers";
export const HERMES_PROVIDER_SOURCE_DICT = "providers_dict";

/**
 * True when the provider was sourced from Hermes' v12+ `providers:` dict —
 * CC Switch renders those read-only and routes edits to Hermes Web UI.
 */
export function isHermesReadOnlyProvider(settingsConfig: unknown): boolean {
  if (!settingsConfig || typeof settingsConfig !== "object") {
    return false;
  }
  const marker = (settingsConfig as Record<string, unknown>)[
    HERMES_PROVIDER_SOURCE_FIELD
  ];
  return marker === HERMES_PROVIDER_SOURCE_DICT;
}
