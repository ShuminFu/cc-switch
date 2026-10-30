import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import JsonEditor from "@/components/JsonEditor";
import { Label } from "@/components/ui/label";
import { providersApi } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query/queries";
import type { OpenCodeProviderPreset } from "@/config/opencodeProviderPresets";
import { parseOmoOtherFieldsObject } from "@/types/omo";
import { OmoFormFields } from "../OmoFormFields";
import { OpenCodeFormFields } from "../OpenCodeFormFields";
import {
  OPENCODE_DEFAULT_CONFIG,
  stripOpencodeModelDrafts,
} from "../helpers/opencodeFormUtils";
import {
  useOmoDraftState,
  useOmoModelSource,
  useOpencodeFormState,
} from "../hooks";
import { ProviderKeyField } from "./ProviderKeyField";
import { useAdapterApiKeyLink, usePublishAdapterHandle } from "./shared";
import type { ProviderFormAdapterContext, ShellFormAdapter } from "./types";
import { useAdditiveProviderKey } from "./useAdditiveProviderKey";

const OPENCODE_CONFIG_PLACEHOLDER = `{
  "npm": "@ai-sdk/openai-compatible",
  "options": {
    "baseURL": "https://your-api-endpoint.com",
    "apiKey": "your-api-key-here"
  },
  "models": {}
}`;

const isOmo = (category: string | undefined) =>
  category === "omo" || category === "omo-slim";

function OpenCodeAdapterBody({ ctx }: { ctx: ProviderFormAdapterContext }) {
  const { t } = useTranslation();
  const { initialData, isEditMode, providerId, category } = ctx;
  const isAnyOmo = isOmo(category);

  const omoSource = useOmoModelSource({ isOmoCategory: isAnyOmo });
  const liveIds = useQuery({
    queryKey: ["opencodeLiveProviderIds"],
    queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
    enabled: !isAnyOmo,
  });
  const storedProviders = useProvidersQuery("opencode");

  const opencodeForm = useOpencodeFormState({
    initialData,
    appId: "opencode",
    providerId,
    onSettingsConfigChange: ctx.setSettingsConfig,
    getSettingsConfig: ctx.getSettingsConfig,
  });

  const omoDraft = useOmoDraftState({
    initialOmoSettings: isOmo(initialData?.category)
      ? (initialData?.settingsConfig as Record<string, unknown> | undefined)
      : undefined,
    isEditMode,
    appId: "opencode",
    category,
  });

  const keyState = useAdditiveProviderKey({
    namespace: "opencode",
    providerKey: opencodeForm.opencodeProviderKey,
    providerId,
    isEditMode,
    storedProviders: {
      data: storedProviders.data?.providers,
      isLoading: storedProviders.isLoading,
      isError: storedProviders.isError,
    },
    liveProviderIds: {
      data: liveIds.data,
      isLoading: !isAnyOmo && liveIds.isLoading,
      isError: !isAnyOmo && liveIds.isError,
    },
  });

  const apiKeyLink = useAdapterApiKeyLink(ctx);

  usePublishAdapterHandle(ctx.handleRef, {
    validate: () => {
      if (isAnyOmo) {
        if (!omoDraft.omoOtherFieldsStr.trim()) return {};
        try {
          if (!parseOmoOtherFieldsObject(omoDraft.omoOtherFieldsStr)) {
            return {
              error: t("omo.jsonMustBeObject", {
                field: t("omo.otherFields", { defaultValue: "Other Config" }),
                defaultValue: "{{field}} must be a JSON object",
              }),
            };
          }
        } catch {
          return {
            error: t("omo.invalidJson", {
              defaultValue: "Other Fields contains invalid JSON",
            }),
          };
        }
        return {};
      }
      const keyError = keyState.getSubmitError();
      if (keyError) return { error: keyError };
      return Object.keys(stripOpencodeModelDrafts(opencodeForm.opencodeModels))
        .length === 0
        ? { issues: [t("opencode.modelsRequired")] }
        : {};
    },
    serialize: (values) => {
      if (!isAnyOmo) {
        return {
          settingsConfig: values.settingsConfig.trim(),
          providerKey: opencodeForm.opencodeProviderKey,
        };
      }
      const omoConfig: Record<string, unknown> = {};
      if (Object.keys(omoDraft.omoAgents).length > 0) {
        omoConfig.agents = omoDraft.omoAgents;
      }
      if (
        category === "omo" &&
        Object.keys(omoDraft.omoCategories).length > 0
      ) {
        omoConfig.categories = omoDraft.omoCategories;
      }
      if (omoDraft.omoOtherFieldsStr.trim()) {
        // Already validated in `validate`.
        const otherFields = parseOmoOtherFieldsObject(
          omoDraft.omoOtherFieldsStr,
        );
        if (otherFields) omoConfig.otherFields = otherFields;
      }
      return {
        settingsConfig: JSON.stringify(omoConfig),
        presetCategory: category,
        providerKey: isEditMode
          ? undefined
          : `${category === "omo" ? "omo" : "omo-slim"}-${crypto
              .randomUUID()
              .slice(0, 8)}`,
      };
    },
    applyPreset: (entry, formValues) => {
      if (!entry) {
        opencodeForm.resetOpencodeState();
        omoDraft.resetOmoDraftState();
        return formValues;
      }
      const preset = entry.preset as OpenCodeProviderPreset;
      if (isOmo(preset.category)) {
        omoDraft.resetOmoDraftState();
        return {
          ...formValues,
          name: preset.category === "omo" ? "OMO" : "OMO Slim",
          settingsConfig: JSON.stringify({}, null, 2),
        };
      }
      opencodeForm.resetOpencodeState(preset.settingsConfig);
      return formValues;
    },
  });

  if (isAnyOmo) {
    return (
      <>
        {ctx.renderBasicFields()}
        <OmoFormFields
          modelOptions={omoSource.omoModelOptions}
          modelVariantsMap={omoSource.omoModelVariantsMap}
          presetMetaMap={omoSource.omoPresetMetaMap}
          agents={omoDraft.omoAgents}
          onAgentsChange={omoDraft.setOmoAgents}
          categories={category === "omo" ? omoDraft.omoCategories : undefined}
          onCategoriesChange={
            category === "omo" ? omoDraft.setOmoCategories : undefined
          }
          otherFieldsStr={omoDraft.omoOtherFieldsStr}
          onOtherFieldsStrChange={omoDraft.setOmoOtherFieldsStr}
          isSlim={category === "omo-slim"}
        />
        <div className="space-y-2">
          <Label>{t("provider.configJson")}</Label>
          <JsonEditor
            value={omoDraft.mergedOmoJsonPreview}
            onChange={() => {}}
            rows={14}
            showValidation={false}
            language="json"
            darkMode={ctx.isDarkMode}
          />
        </div>
      </>
    );
  }

  return (
    <>
      {ctx.renderBasicFields(
        <ProviderKeyField
          id="opencode-key"
          value={opencodeForm.opencodeProviderKey}
          onChange={opencodeForm.setOpencodeProviderKey}
          state={keyState}
        />,
      )}
      <OpenCodeFormFields
        npm={opencodeForm.opencodeNpm}
        onNpmChange={opencodeForm.handleOpencodeNpmChange}
        apiKey={opencodeForm.opencodeApiKey}
        onApiKeyChange={opencodeForm.handleOpencodeApiKeyChange}
        category={category}
        shouldShowApiKeyLink={apiKeyLink.shouldShowApiKeyLink}
        websiteUrl={apiKeyLink.websiteUrl}
        isPartner={apiKeyLink.isPartner}
        partnerPromotionKey={apiKeyLink.partnerPromotionKey}
        baseUrl={opencodeForm.opencodeBaseUrl}
        onBaseUrlChange={opencodeForm.handleOpencodeBaseUrlChange}
        headers={opencodeForm.opencodeHeaders}
        onHeadersChange={opencodeForm.handleOpencodeHeadersChange}
        models={opencodeForm.opencodeModels}
        onModelsChange={opencodeForm.handleOpencodeModelsChange}
        extraOptions={opencodeForm.opencodeExtraOptions}
        onExtraOptionsChange={opencodeForm.handleOpencodeExtraOptionsChange}
      />
      <div className="space-y-2">
        <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
        <JsonEditor
          value={ctx.settingsConfig}
          onChange={ctx.setSettingsConfig}
          placeholder={OPENCODE_CONFIG_PLACEHOLDER}
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

export const opencodeFormAdapter: ShellFormAdapter = {
  kind: "shell",
  Body: OpenCodeAdapterBody,
  defaultSettingsConfig: OPENCODE_DEFAULT_CONFIG,
  showPricing: () => false,
  hasCommonConfig: false,
};
