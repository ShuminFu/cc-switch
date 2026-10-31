import {
  useCallback,
  useLayoutEffect,
  useState,
  type MutableRefObject,
} from "react";
import { useWatch } from "react-hook-form";
import type { TFunction } from "i18next";
import {
  buildLocalProxyRequestOverrides,
  formatRequestOverrideObject,
} from "@/lib/requestOverrides";
import type { CustomEndpoint, ProviderCategory, ProviderMeta } from "@/types";
import { getProviderCodec, parseSettingsConfig } from "@/lib/providerCodecs";
import { useApiKeyLink } from "../hooks/useApiKeyLink";
import type {
  ProviderFormAdapterContext,
  ProviderFormAdapterHandle,
} from "./types";

/** Publish the adapter's submit handlers to the shell after every render. */
export function usePublishAdapterHandle(
  ref: MutableRefObject<ProviderFormAdapterHandle | null>,
  handle: ProviderFormAdapterHandle,
) {
  useLayoutEffect(() => {
    ref.current = handle;
  });
}

export const isPlainJsonObject = (value: unknown) =>
  !!value && typeof value === "object" && !Array.isArray(value);

/** Category that has user-supplied endpoint / key fields to check. */
export const needsEndpointAndKey = (category: ProviderCategory | undefined) =>
  category !== "official" && category !== "cloud_provider";

export const endpointRequiredIssue = (t: TFunction) =>
  t("providerForm.endpointRequired", {
    defaultValue: "API endpoint is required for non-official providers",
  });

export const apiKeyRequiredIssue = (t: TFunction) =>
  t("providerForm.apiKeyRequired", {
    defaultValue: "API Key is required for non-official providers",
  });

/** API-key link / partner info of the selected preset for this app. */
export function useAdapterApiKeyLink(ctx: ProviderFormAdapterContext) {
  const websiteUrl =
    useWatch({ control: ctx.form.control, name: "websiteUrl" }) ?? "";
  return useApiKeyLink({
    appId: ctx.appId,
    category: ctx.category,
    selectedPresetId: ctx.selectedPresetId,
    presetEntries: ctx.presetEntries as Parameters<
      typeof useApiKeyLink
    >[0]["presetEntries"],
    formWebsiteUrl: websiteUrl,
  });
}

/**
 * Whether the API key input is shown: always for a chosen preset / custom in
 * add mode, and in edit mode for non-official providers or configs that carry
 * a key slot.
 */
export function shouldShowApiKeyInput(
  ctx: ProviderFormAdapterContext,
): boolean {
  return (
    ctx.selectedPresetId !== null ||
    (ctx.isEditMode && needsEndpointAndKey(ctx.category)) ||
    (ctx.isEditMode &&
      getProviderCodec(ctx.appId).hasApiKeyField(
        parseSettingsConfig(ctx.settingsConfig),
      ))
  );
}

/**
 * Speed-test panel state shared by Claude / Codex / Gemini: auto-select flag
 * and, in add mode, the draft custom endpoints that become
 * `meta.custom_endpoints` on save.
 */
export function useEndpointDraftState(ctx: ProviderFormAdapterContext) {
  const [draftCustomEndpoints, setDraftCustomEndpoints] = useState<string[]>(
    [],
  );
  const [endpointAutoSelect, setEndpointAutoSelect] = useState<boolean>(
    () => ctx.initialData?.meta?.endpointAutoSelect ?? true,
  );

  const toMeta = useCallback((): Partial<ProviderMeta> => {
    const meta: Partial<ProviderMeta> = { endpointAutoSelect };
    if (!ctx.isEditMode && draftCustomEndpoints.length > 0) {
      const now = Date.now();
      meta.custom_endpoints = Object.fromEntries(
        draftCustomEndpoints.map((url): [string, CustomEndpoint] => [
          url,
          { url, addedAt: now, lastUsed: undefined },
        ]),
      );
    }
    return meta;
  }, [ctx.isEditMode, draftCustomEndpoints, endpointAutoSelect]);

  return {
    draftCustomEndpoints,
    setDraftCustomEndpoints,
    endpointAutoSelect,
    setEndpointAutoSelect,
    toMeta,
  };
}

/**
 * Local-proxy request customisation shared by Claude and Codex: custom
 * User-Agent plus header/body override JSON.
 */
export function useLocalProxyOverridesState(ctx: ProviderFormAdapterContext) {
  const meta = ctx.initialData?.meta;
  const [customUserAgent, setCustomUserAgent] = useState<string>(
    () => meta?.customUserAgent ?? "",
  );
  const [headersOverride, setHeadersOverride] = useState<string>(() =>
    formatRequestOverrideObject(meta?.localProxyRequestOverrides?.headers),
  );
  const [bodyOverride, setBodyOverride] = useState<string>(() =>
    formatRequestOverrideObject(meta?.localProxyRequestOverrides?.body),
  );

  const applies = ctx.category !== "official";

  const build = useCallback(
    () =>
      applies
        ? buildLocalProxyRequestOverrides(headersOverride, bodyOverride)
        : {},
    [applies, headersOverride, bodyOverride],
  );

  /** Hard error text when the override JSON is invalid. */
  const getError = useCallback(
    (t: TFunction): string | null => {
      const result = build();
      if (!result.error) return null;
      return t("providerForm.localProxyRequestOverridesInvalid", {
        defaultValue: "Local proxy request overrides format error: {{error}}",
        error: result.error,
      });
    },
    [build],
  );

  const toMeta = useCallback(
    (): Partial<ProviderMeta> => ({
      customUserAgent: applies
        ? customUserAgent.trim() || undefined
        : undefined,
      localProxyRequestOverrides: applies ? build().overrides : undefined,
    }),
    [applies, build, customUserAgent],
  );

  return {
    customUserAgent,
    setCustomUserAgent,
    headersOverride,
    setHeadersOverride,
    bodyOverride,
    setBodyOverride,
    getError,
    toMeta,
  };
}

/** Patch the RHF settingsConfig JSON in place (ignores unparsable text). */
export function updateSettingsJson(
  ctx: ProviderFormAdapterContext,
  updater: (config: Record<string, unknown>) => void,
) {
  const config = parseSettingsConfig(ctx.getSettingsConfig() || "{}");
  if (!config) return;
  updater(config);
  ctx.setSettingsConfig(JSON.stringify(config, null, 2));
}
