import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import type { GeminiProviderPreset } from "@/config/geminiProviderPresets";
import GeminiConfigEditor from "../GeminiConfigEditor";
import { GeminiFormFields } from "../GeminiFormFields";
import { GEMINI_DEFAULT_CONFIG } from "../helpers/opencodeFormUtils";
import {
  useGeminiCommonConfig,
  useGeminiConfigState,
  useSpeedTestEndpoints,
} from "../hooks";
import {
  apiKeyRequiredIssue,
  endpointRequiredIssue,
  isPlainJsonObject,
  needsEndpointAndKey,
  shouldShowApiKeyInput,
  updateSettingsJson,
  useAdapterApiKeyLink,
  useEndpointDraftState,
  usePublishAdapterHandle,
} from "./shared";
import type { ProviderFormAdapterContext, ShellFormAdapter } from "./types";

type GeminiEnvKey =
  | "GEMINI_API_KEY"
  | "GOOGLE_GEMINI_BASE_URL"
  | "GEMINI_MODEL";

function GeminiAdapterBody({ ctx }: { ctx: ProviderFormAdapterContext }) {
  const { t } = useTranslation();
  const { initialData, isEditMode, category, selectedPresetId } = ctx;
  const [isEndpointModalOpen, setIsEndpointModalOpen] = useState(false);
  const endpoints = useEndpointDraftState(ctx);

  const {
    geminiEnv,
    geminiConfig,
    geminiApiKey,
    geminiBaseUrl,
    geminiModel,
    envError,
    configError,
    handleGeminiApiKeyChange: setApiKeyInEnv,
    handleGeminiBaseUrlChange: setBaseUrlInEnv,
    handleGeminiModelChange: setModelInEnv,
    handleGeminiEnvChange,
    handleGeminiConfigChange,
    resetGeminiConfig,
    envStringToObj,
    envObjToString,
  } = useGeminiConfigState({ initialData });

  // Mirror structured edits into the RHF settingsConfig as well.
  const updateEnvField = useCallback(
    (key: GeminiEnvKey, value: string) => {
      updateSettingsJson(ctx, (config) => {
        const env =
          config.env && typeof config.env === "object"
            ? (config.env as Record<string, unknown>)
            : {};
        env[key] = value;
        config.env = env;
      });
    },
    [ctx],
  );

  const handleApiKeyChange = useCallback(
    (key: string) => {
      setApiKeyInEnv(key);
      updateEnvField("GEMINI_API_KEY", key.trim());
    },
    [setApiKeyInEnv, updateEnvField],
  );
  const handleBaseUrlChange = useCallback(
    (url: string) => {
      setBaseUrlInEnv(url);
      updateEnvField("GOOGLE_GEMINI_BASE_URL", url.trim().replace(/\/+$/, ""));
    },
    [setBaseUrlInEnv, updateEnvField],
  );
  const handleModelChange = useCallback(
    (model: string) => {
      setModelInEnv(model);
      updateEnvField("GEMINI_MODEL", model.trim());
    },
    [setModelInEnv, updateEnvField],
  );

  const commonConfig = useGeminiCommonConfig({
    envValue: geminiEnv,
    onEnvChange: handleGeminiEnvChange,
    envStringToObj,
    envObjToString,
    initialData,
    initialEnabled: initialData?.meta?.commonConfigEnabled,
    selectedPresetId: selectedPresetId ?? undefined,
  });

  const apiKeyLink = useAdapterApiKeyLink(ctx);
  const speedTestEndpoints = useSpeedTestEndpoints({
    appId: "gemini",
    selectedPresetId,
    baseUrl: geminiBaseUrl,
    initialData,
    draftCustomEndpoints: isEditMode
      ? undefined
      : endpoints.draftCustomEndpoints,
  });

  const getEditorError = (): string | null => {
    if (envError) return envError;
    if (!geminiConfig.trim()) return null;
    let parsed: unknown;
    try {
      parsed = JSON.parse(geminiConfig);
    } catch {
      return t("providerForm.geminiConfigInvalid", {
        defaultValue:
          "Gemini config.json is not valid JSON. Fix it before saving.",
      });
    }
    return isPlainJsonObject(parsed)
      ? null
      : t("providerForm.geminiConfigNotObject", {
          defaultValue: "Gemini config.json must be a JSON object.",
        });
  };

  usePublishAdapterHandle(ctx.handleRef, {
    validate: () => {
      const error = getEditorError();
      if (error) return { error };
      const issues: string[] = [];
      if (needsEndpointAndKey(category)) {
        if (!geminiBaseUrl.trim()) issues.push(endpointRequiredIssue(t));
        if (!geminiApiKey.trim()) issues.push(apiKeyRequiredIssue(t));
      }
      return { issues };
    },
    serialize: () => {
      let configObj: unknown;
      try {
        configObj = geminiConfig.trim() ? JSON.parse(geminiConfig) : {};
      } catch {
        return null;
      }
      return {
        settingsConfig: JSON.stringify({
          env: envStringToObj(geminiEnv),
          config: configObj,
        }),
        meta: {
          ...endpoints.toMeta(),
          commonConfigEnabled: commonConfig.useCommonConfig,
        },
      };
    },
    applyPreset: (entry, formValues) => {
      if (!entry) {
        resetGeminiConfig({}, {});
        return formValues;
      }
      const preset = entry.preset as GeminiProviderPreset;
      const settings = (preset.settingsConfig ?? {}) as {
        env?: Record<string, unknown>;
        config?: Record<string, unknown>;
      };
      resetGeminiConfig(settings.env ?? {}, settings.config ?? {});
      return formValues;
    },
  });

  return (
    <>
      {ctx.renderBasicFields()}

      <GeminiFormFields
        providerId={ctx.providerId}
        shouldShowApiKey={shouldShowApiKeyInput(ctx)}
        apiKey={geminiApiKey}
        onApiKeyChange={handleApiKeyChange}
        category={category}
        shouldShowApiKeyLink={apiKeyLink.shouldShowApiKeyLink}
        websiteUrl={apiKeyLink.websiteUrl}
        isPartner={apiKeyLink.isPartner}
        partnerPromotionKey={apiKeyLink.partnerPromotionKey}
        shouldShowSpeedTest={needsEndpointAndKey(category)}
        baseUrl={geminiBaseUrl}
        onBaseUrlChange={handleBaseUrlChange}
        isEndpointModalOpen={isEndpointModalOpen}
        onEndpointModalToggle={setIsEndpointModalOpen}
        onCustomEndpointsChange={endpoints.setDraftCustomEndpoints}
        autoSelect={endpoints.endpointAutoSelect}
        onAutoSelectChange={endpoints.setEndpointAutoSelect}
        shouldShowModelField={true}
        model={geminiModel}
        onModelChange={handleModelChange}
        speedTestEndpoints={speedTestEndpoints}
      />

      <GeminiConfigEditor
        envValue={geminiEnv}
        configValue={geminiConfig}
        onEnvChange={handleGeminiEnvChange}
        onConfigChange={handleGeminiConfigChange}
        useCommonConfig={commonConfig.useCommonConfig}
        onCommonConfigToggle={commonConfig.handleCommonConfigToggle}
        commonConfigSnippet={commonConfig.commonConfigSnippet}
        onCommonConfigSnippetChange={
          commonConfig.handleCommonConfigSnippetChange
        }
        onCommonConfigErrorClear={commonConfig.clearCommonConfigError}
        commonConfigError={commonConfig.commonConfigError}
        envError={envError}
        configError={configError}
        onExtract={commonConfig.handleExtract}
        isExtracting={commonConfig.isExtracting}
      />
      {ctx.settingsConfigErrorField}
    </>
  );
}

export const geminiFormAdapter: ShellFormAdapter = {
  kind: "shell",
  Body: GeminiAdapterBody,
  defaultSettingsConfig: GEMINI_DEFAULT_CONFIG,
  showPricing: () => true,
  hasCommonConfig: true,
};
