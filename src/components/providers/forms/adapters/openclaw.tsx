import { useState } from "react";
import { useTranslation } from "react-i18next";
import JsonEditor from "@/components/JsonEditor";
import { Label } from "@/components/ui/label";
import { useOpenClawLiveProviderIds } from "@/hooks/useOpenClaw";
import { useProvidersQuery } from "@/lib/query/queries";
import {
  rebaseOpenClawSuggestedDefaults,
  type OpenClawProviderPreset,
  type OpenClawSuggestedDefaults,
} from "@/config/openclawProviderPresets";
import { OpenClawFormFields } from "../OpenClawFormFields";
import { OPENCLAW_DEFAULT_CONFIG } from "../helpers/opencodeFormUtils";
import { useOpenclawFormState } from "../hooks";
import { ProviderKeyField } from "./ProviderKeyField";
import { useAdapterApiKeyLink, usePublishAdapterHandle } from "./shared";
import type { ProviderFormAdapterContext, ShellFormAdapter } from "./types";
import { useAdditiveProviderKey } from "./useAdditiveProviderKey";

const OPENCLAW_CONFIG_PLACEHOLDER = `{
  "baseUrl": "https://api.example.com/v1",
  "apiKey": "your-api-key-here",
  "api": "openai-completions",
  "models": []
}`;

function OpenClawAdapterBody({ ctx }: { ctx: ProviderFormAdapterContext }) {
  const { t } = useTranslation();
  const { initialData, isEditMode, providerId, category } = ctx;
  const [suggestedDefaults, setSuggestedDefaults] = useState<
    OpenClawSuggestedDefaults | undefined
  >();

  const openclawForm = useOpenclawFormState({
    initialData,
    appId: "openclaw",
    providerId,
    onSettingsConfigChange: ctx.setSettingsConfig,
    getSettingsConfig: ctx.getSettingsConfig,
  });
  const liveIds = useOpenClawLiveProviderIds(true);
  const storedProviders = useProvidersQuery("openclaw");
  const keyState = useAdditiveProviderKey({
    namespace: "openclaw",
    providerKey: openclawForm.openclawProviderKey,
    providerId,
    isEditMode,
    storedProviders: {
      data: storedProviders.data?.providers,
      isLoading: storedProviders.isLoading,
      isError: storedProviders.isError,
    },
    liveProviderIds: {
      data: liveIds.data,
      isLoading: liveIds.isLoading,
      isError: liveIds.isError,
    },
  });
  const apiKeyLink = useAdapterApiKeyLink(ctx);

  usePublishAdapterHandle(ctx.handleRef, {
    validate: () => ({ error: keyState.getSubmitError() }),
    serialize: (values) => {
      const providerKey = openclawForm.openclawProviderKey;
      return {
        settingsConfig: values.settingsConfig.trim(),
        providerKey,
        // Align the preset's model refs with the submitted provider key.
        suggestedDefaults:
          suggestedDefaults && providerKey
            ? rebaseOpenClawSuggestedDefaults(suggestedDefaults, providerKey)
            : suggestedDefaults,
      };
    },
    applyPreset: (entry, formValues) => {
      if (!entry) {
        setSuggestedDefaults(undefined);
        openclawForm.resetOpenclawState();
        return formValues;
      }
      const preset = entry.preset as OpenClawProviderPreset;
      setSuggestedDefaults(preset.suggestedDefaults);
      openclawForm.resetOpenclawState(preset.settingsConfig);
      return formValues;
    },
  });

  return (
    <>
      {ctx.renderBasicFields(
        <ProviderKeyField
          id="openclaw-key"
          value={openclawForm.openclawProviderKey}
          onChange={openclawForm.setOpenclawProviderKey}
          state={keyState}
        />,
      )}
      <OpenClawFormFields
        baseUrl={openclawForm.openclawBaseUrl}
        onBaseUrlChange={openclawForm.handleOpenclawBaseUrlChange}
        apiKey={openclawForm.openclawApiKey}
        onApiKeyChange={openclawForm.handleOpenclawApiKeyChange}
        category={category}
        shouldShowApiKeyLink={apiKeyLink.shouldShowApiKeyLink}
        websiteUrl={apiKeyLink.websiteUrl}
        isPartner={apiKeyLink.isPartner}
        partnerPromotionKey={apiKeyLink.partnerPromotionKey}
        api={openclawForm.openclawApi}
        onApiChange={openclawForm.handleOpenclawApiChange}
        models={openclawForm.openclawModels}
        onModelsChange={openclawForm.handleOpenclawModelsChange}
        userAgent={openclawForm.openclawUserAgent}
        onUserAgentChange={openclawForm.handleOpenclawUserAgentChange}
      />
      <div className="space-y-2">
        <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
        <JsonEditor
          value={ctx.settingsConfig}
          onChange={ctx.setSettingsConfig}
          placeholder={OPENCLAW_CONFIG_PLACEHOLDER}
          rows={14}
          showValidation={true}
          language="json"
          darkMode={ctx.isDarkMode}
        />
      </div>
      {ctx.settingsConfigErrorField}
    </>
  );
}

export const openclawFormAdapter: ShellFormAdapter = {
  kind: "shell",
  Body: OpenClawAdapterBody,
  defaultSettingsConfig: OPENCLAW_DEFAULT_CONFIG,
  showPricing: () => false,
  hasCommonConfig: false,
};
