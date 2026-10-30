import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderMeta } from "@/types";
import type { PresetEntry } from "@/config/presetRegistry";
import type { PricingModelSourceOption } from "../ProviderAdvancedConfig";
import { normalizePricingSource } from "../helpers/opencodeFormUtils";
import type {
  AdapterSerialization,
  ProviderFormInitialData,
  ProviderFormValues,
} from "../adapters/types";

/**
 * Meta fields owned by the provider form. They are reset on every save and
 * re-populated by the shell and the active adapter, so a stale value from
 * another configuration (e.g. a Codex chat-reasoning block after switching
 * the API format) is dropped.
 */
export const FORM_MANAGED_META_KEYS = [
  "commonConfigEnabled",
  "claudeDesktopMode",
  "authBinding",
  "githubAccountId",
  "codexFastMode",
  "codexChatReasoning",
  "promptCacheRouting",
  "customUserAgent",
  "localProxyRequestOverrides",
  "apiFormat",
  "apiKeyField",
  "impersonateClaudeCode",
  "maxOutputTokens",
  "isFullUrl",
] as const satisfies readonly (keyof ProviderMeta)[];

export interface PricingConfig {
  enabled: boolean;
  costMultiplier?: string;
  pricingModelSource: PricingModelSourceOption;
}

export const initialPricingConfig = (
  initialData?: ProviderFormInitialData,
): PricingConfig => ({
  enabled:
    initialData?.meta?.costMultiplier !== undefined ||
    initialData?.meta?.pricingModelSource !== undefined,
  costMultiplier: initialData?.meta?.costMultiplier,
  pricingModelSource: normalizePricingSource(
    initialData?.meta?.pricingModelSource,
  ),
});

interface BuildProviderPayloadInput {
  values: ProviderFormData;
  result: AdapterSerialization;
  activePreset: PresetEntry | null;
  initialData?: ProviderFormInitialData;
  isEditMode: boolean;
  pricing: PricingConfig;
}

/** Combine the shell's form values with the adapter's serialisation. */
export function buildProviderPayload({
  values,
  result,
  activePreset,
  initialData,
  isEditMode,
  pricing,
}: BuildProviderPayloadInput): ProviderFormValues {
  const payload: ProviderFormValues = {
    ...values,
    name: values.name.trim(),
    websiteUrl: values.websiteUrl?.trim() ?? "",
    settingsConfig: result.settingsConfig,
  };
  if (result.providerKey !== undefined) {
    payload.providerKey = result.providerKey;
  }
  if (result.presetCategory) payload.presetCategory = result.presetCategory;
  if (activePreset) {
    payload.presetId = activePreset.id;
    if (activePreset.preset.category) {
      payload.presetCategory = activePreset.preset.category;
    }
    if (activePreset.preset.isPartner) {
      payload.isPartner = activePreset.preset.isPartner;
    }
  }
  if (result.suggestedDefaults) {
    payload.suggestedDefaults = result.suggestedDefaults;
  }

  const meta: ProviderMeta = { ...(initialData?.meta ?? {}) };
  for (const key of FORM_MANAGED_META_KEYS) {
    meta[key] = undefined;
  }
  Object.assign(meta, {
    endpointAutoSelect: initialData?.meta?.endpointAutoSelect ?? true,
    providerType: initialData?.meta?.providerType,
    costMultiplier: pricing.enabled ? pricing.costMultiplier : undefined,
    pricingModelSource:
      pricing.enabled && pricing.pricingModelSource !== "inherit"
        ? pricing.pricingModelSource
        : undefined,
    ...result.meta,
  } satisfies Partial<ProviderMeta>);
  if (meta.codexFastMode === undefined) delete meta.codexFastMode;

  // Partner badge / promotion key come from the chosen preset and are
  // persisted on every add (FE-PROV-43).
  if (!isEditMode && activePreset?.preset.isPartner) meta.isPartner = true;
  if (!isEditMode && activePreset?.preset.partnerPromotionKey) {
    meta.partnerPromotionKey = activePreset.preset.partnerPromotionKey;
  }
  payload.meta = meta;
  return payload;
}
