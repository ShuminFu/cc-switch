import { useTranslation } from "react-i18next";
import JsonEditor from "@/components/JsonEditor";
import { Label } from "@/components/ui/label";
import { useHermesLiveProviderIds } from "@/hooks/useHermes";
import { useProvidersQuery } from "@/lib/query/queries";
import { HERMES_DEFAULT_CONFIG } from "@/lib/providerCodecs";
import type { HermesProviderPreset } from "@/config/hermesProviderPresets";
import { HermesFormFields } from "../HermesFormFields";
import { useHermesFormState } from "../hooks";
import { ProviderKeyField } from "./ProviderKeyField";
import { useAdapterApiKeyLink, usePublishAdapterHandle } from "./shared";
import type { ProviderFormAdapterContext, ShellFormAdapter } from "./types";
import { useAdditiveProviderKey } from "./useAdditiveProviderKey";

const HERMES_CONFIG_PLACEHOLDER = `{
  "name": "my-provider",
  "base_url": "https://api.example.com/v1",
  "api_key": ""
}`;

function HermesAdapterBody({ ctx }: { ctx: ProviderFormAdapterContext }) {
  const { t } = useTranslation();
  const { initialData, isEditMode, providerId, category } = ctx;

  const hermesForm = useHermesFormState({
    initialData,
    appId: "hermes",
    providerId,
    onSettingsConfigChange: ctx.setSettingsConfig,
    getSettingsConfig: ctx.getSettingsConfig,
  });
  const liveIds = useHermesLiveProviderIds(true);
  const storedProviders = useProvidersQuery("hermes");
  const keyState = useAdditiveProviderKey({
    namespace: "hermes.form",
    providerKey: hermesForm.hermesProviderKey,
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
    serialize: (values) => ({
      settingsConfig: values.settingsConfig.trim(),
      providerKey: hermesForm.hermesProviderKey,
    }),
    applyPreset: (entry, formValues) => {
      hermesForm.resetHermesState(
        entry
          ? (entry.preset as HermesProviderPreset).settingsConfig
          : undefined,
      );
      return formValues;
    },
  });

  return (
    <>
      {ctx.renderBasicFields(
        <ProviderKeyField
          id="hermes-key"
          value={hermesForm.hermesProviderKey}
          onChange={hermesForm.setHermesProviderKey}
          state={keyState}
        />,
      )}
      <HermesFormFields
        baseUrl={hermesForm.hermesBaseUrl}
        onBaseUrlChange={hermesForm.handleHermesBaseUrlChange}
        apiKey={hermesForm.hermesApiKey}
        onApiKeyChange={hermesForm.handleHermesApiKeyChange}
        category={category}
        shouldShowApiKeyLink={apiKeyLink.shouldShowApiKeyLink}
        websiteUrl={apiKeyLink.websiteUrl}
        isPartner={apiKeyLink.isPartner}
        partnerPromotionKey={apiKeyLink.partnerPromotionKey}
        apiMode={hermesForm.hermesApiMode}
        onApiModeChange={hermesForm.handleHermesApiModeChange}
        models={hermesForm.hermesModels}
        onModelsChange={hermesForm.handleHermesModelsChange}
        rateLimitDelay={hermesForm.hermesRateLimitDelay}
        onRateLimitDelayChange={hermesForm.handleHermesRateLimitDelayChange}
      />
      <div className="space-y-2">
        <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
        <JsonEditor
          value={ctx.settingsConfig}
          onChange={ctx.setSettingsConfig}
          placeholder={HERMES_CONFIG_PLACEHOLDER}
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

export const hermesFormAdapter: ShellFormAdapter = {
  kind: "shell",
  Body: HermesAdapterBody,
  defaultSettingsConfig: HERMES_DEFAULT_CONFIG,
  showPricing: () => false,
  hasCommonConfig: false,
};
