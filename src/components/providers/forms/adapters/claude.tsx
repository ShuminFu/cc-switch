import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ClaudeApiFormat, ClaudeApiKeyField, ProviderMeta } from "@/types";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { getProviderCodec, parseSettingsConfig } from "@/lib/providerCodecs";
import { applyTemplateValues } from "@/utils/providerConfigUtils";
import { ClaudeFormFields } from "../ClaudeFormFields";
import { CommonConfigEditor } from "../CommonConfigEditor";
import { CLAUDE_DEFAULT_CONFIG } from "../helpers/opencodeFormUtils";
import {
  useApiKeyState,
  useCodexOauth,
  useCommonConfigSnippet,
  useCopilotAuth,
  useModelState,
  useSpeedTestEndpoints,
  useTemplateValues,
} from "../hooks";
import {
  apiKeyRequiredIssue,
  endpointRequiredIssue,
  needsEndpointAndKey,
  updateSettingsJson,
  useAdapterApiKeyLink,
  useEndpointDraftState,
  useLocalProxyOverridesState,
  usePublishAdapterHandle,
} from "./shared";
import type { ProviderFormAdapterContext, ShellFormAdapter } from "./types";

const inferApiKeyField = (
  ctx: ProviderFormAdapterContext,
): ClaudeApiKeyField => {
  const meta = ctx.initialData?.meta;
  if (meta?.apiKeyField) return meta.apiKeyField;
  const env = (ctx.initialData?.settingsConfig as Record<string, unknown>)
    ?.env as Record<string, unknown> | undefined;
  return env?.ANTHROPIC_API_KEY !== undefined
    ? "ANTHROPIC_API_KEY"
    : "ANTHROPIC_AUTH_TOKEN";
};

function ClaudeAdapterBody({ ctx }: { ctx: ProviderFormAdapterContext }) {
  const { t } = useTranslation();
  const { initialData, isEditMode, category, settingsConfig } = ctx;
  const initialMeta = initialData?.meta;

  const [apiKeyField, setApiKeyField] = useState<ClaudeApiKeyField>(() =>
    inferApiKeyField(ctx),
  );
  const [apiFormat, setApiFormat] = useState<ClaudeApiFormat>(
    () => initialMeta?.apiFormat ?? "anthropic",
  );
  const [isFullUrl, setIsFullUrl] = useState<boolean>(
    () => initialMeta?.isFullUrl ?? false,
  );
  const [isEndpointModalOpen, setIsEndpointModalOpen] = useState(false);
  const [isCommonConfigModalOpen, setIsCommonConfigModalOpen] = useState(false);
  const [selectedGitHubAccountId, setSelectedGitHubAccountId] = useState<
    string | null
  >(() => resolveManagedAccountId(initialMeta, "github_copilot"));
  const [selectedCodexAccountId, setSelectedCodexAccountId] = useState<
    string | null
  >(() => resolveManagedAccountId(initialMeta, "codex_oauth"));
  const [codexFastMode, setCodexFastMode] = useState<boolean>(
    () => initialMeta?.codexFastMode ?? false,
  );

  const endpoints = useEndpointDraftState(ctx);
  const overrides = useLocalProxyOverridesState(ctx);

  const { apiKey, handleApiKeyChange, showApiKey } = useApiKeyState({
    initialConfig: settingsConfig,
    onConfigChange: ctx.setSettingsConfig,
    selectedPresetId: ctx.selectedPresetId,
    category,
    appType: "claude",
    apiKeyField,
  });

  // Base URL is derived from the live config; official presets have none.
  const baseUrl = useMemo(() => {
    if (category === "official") return "";
    return (
      getProviderCodec("claude").getEndpoint(
        parseSettingsConfig(settingsConfig),
      ) ?? ""
    );
  }, [category, settingsConfig]);

  const handleBaseUrlChange = useCallback(
    (url: string) => {
      updateSettingsJson(ctx, (config) => {
        const env = (config.env ?? {}) as Record<string, unknown>;
        env.ANTHROPIC_BASE_URL = url.trim();
        config.env = env;
      });
    },
    [ctx],
  );

  const handleApiKeyFieldChange = useCallback(
    (field: ClaudeApiKeyField) => {
      const prev = apiKeyField;
      setApiKeyField(field);
      // Move the key value to the newly chosen env variable.
      const config = parseSettingsConfig(ctx.getSettingsConfig() || "{}");
      const env = config?.env as Record<string, unknown> | undefined;
      if (config && env && prev in env) {
        const value = env[prev];
        delete env[prev];
        env[field] = value;
        ctx.setSettingsConfig(JSON.stringify(config, null, 2));
      }
    },
    [apiKeyField, ctx],
  );

  const models = useModelState({
    settingsConfig,
    onConfigChange: ctx.setSettingsConfig,
  });

  const {
    templateValues,
    templateValueEntries,
    selectedPreset: templatePreset,
    handleTemplateValueChange,
    validateTemplateValues,
  } = useTemplateValues({
    selectedPresetId: ctx.selectedPresetId,
    presetEntries: ctx.presetEntries as Parameters<
      typeof useTemplateValues
    >[0]["presetEntries"],
    settingsConfig,
    onConfigChange: ctx.setSettingsConfig,
  });

  const commonConfig = useCommonConfigSnippet({
    settingsConfig,
    onConfigChange: ctx.setSettingsConfig,
    initialData,
    initialEnabled: initialMeta?.commonConfigEnabled,
    selectedPresetId: ctx.selectedPresetId ?? undefined,
    enabled: true,
  });

  const { isAuthenticated: isCopilotAuthenticated } = useCopilotAuth();
  const { isAuthenticated: isCodexOauthAuthenticated } = useCodexOauth();

  const isCopilotProvider =
    templatePreset?.providerType === "github_copilot" ||
    initialMeta?.providerType === "github_copilot" ||
    baseUrl.includes("githubcopilot.com");
  const isCodexOauthProvider =
    templatePreset?.providerType === "codex_oauth" ||
    initialMeta?.providerType === "codex_oauth";

  const apiKeyLink = useAdapterApiKeyLink(ctx);
  const speedTestEndpoints = useSpeedTestEndpoints({
    appId: "claude",
    selectedPresetId: ctx.selectedPresetId,
    baseUrl,
    initialData,
    draftCustomEndpoints: isEditMode
      ? undefined
      : endpoints.draftCustomEndpoints,
  });

  usePublishAdapterHandle(ctx.handleRef, {
    validate: () => {
      const overridesError = overrides.getError(t);
      if (overridesError) return { error: overridesError };

      const issues: string[] = [];
      if (templateValueEntries.length > 0) {
        const validation = validateTemplateValues();
        if (!validation.isValid && validation.missingField) {
          issues.push(
            t("providerForm.fillParameter", {
              label: validation.missingField.label,
              defaultValue: "Please fill in {{label}}",
            }),
          );
        }
      }

      // OAuth providers cannot be created without a token.
      if (isCopilotProvider && !isCopilotAuthenticated) {
        return {
          error: t("copilot.loginRequired", {
            defaultValue: "Please login to GitHub Copilot first",
          }),
        };
      }
      if (isCodexOauthProvider && !isCodexOauthAuthenticated) {
        return {
          error: t("codexOauth.loginRequired", {
            defaultValue: "Please sign in to ChatGPT first",
          }),
        };
      }

      if (needsEndpointAndKey(category)) {
        if (!isCodexOauthProvider && !baseUrl.trim()) {
          issues.push(endpointRequiredIssue(t));
        }
        if (!isCopilotProvider && !isCodexOauthProvider && !apiKey.trim()) {
          issues.push(apiKeyRequiredIssue(t));
        }
      }
      return { issues };
    },
    serialize: (values) => {
      const isOfficial = category === "official";
      const meta: Partial<ProviderMeta> = {
        ...endpoints.toMeta(),
        ...overrides.toMeta(),
        commonConfigEnabled: commonConfig.useCommonConfig,
        providerType: templatePreset?.providerType || initialMeta?.providerType,
        authBinding: isCopilotProvider
          ? {
              source: "managed_account",
              authProvider: "github_copilot",
              accountId: selectedGitHubAccountId ?? undefined,
            }
          : isCodexOauthProvider
            ? {
                source: "managed_account",
                authProvider: "codex_oauth",
                accountId: selectedCodexAccountId ?? undefined,
              }
            : undefined,
        githubAccountId:
          isCopilotProvider && selectedGitHubAccountId
            ? selectedGitHubAccountId
            : undefined,
        codexFastMode: isCodexOauthProvider ? codexFastMode : undefined,
        apiFormat: isOfficial ? undefined : apiFormat,
        apiKeyField:
          !isOfficial && apiKeyField !== "ANTHROPIC_AUTH_TOKEN"
            ? apiKeyField
            : undefined,
        isFullUrl: !isOfficial && isFullUrl ? true : undefined,
      };
      return { settingsConfig: values.settingsConfig.trim(), meta };
    },
    applyPreset: (entry, formValues) => {
      if (!entry) return formValues;
      const preset = entry.preset as ProviderPreset;
      setApiFormat(preset.apiFormat ?? "anthropic");
      setApiKeyField(preset.apiKeyField ?? "ANTHROPIC_AUTH_TOKEN");
      setIsFullUrl(false);
      const config = applyTemplateValues(
        preset.settingsConfig,
        preset.templateValues,
      );
      return { ...formValues, settingsConfig: JSON.stringify(config, null, 2) };
    },
  });

  return (
    <>
      {ctx.renderBasicFields()}

      <ClaudeFormFields
        providerId={ctx.providerId}
        shouldShowApiKey={
          (category !== "cloud_provider" ||
            getProviderCodec("claude").hasApiKeyField(
              parseSettingsConfig(settingsConfig),
            )) &&
          showApiKey(settingsConfig, isEditMode)
        }
        apiKey={apiKey}
        onApiKeyChange={handleApiKeyChange}
        category={category}
        shouldShowApiKeyLink={apiKeyLink.shouldShowApiKeyLink}
        websiteUrl={apiKeyLink.websiteUrl}
        isPartner={apiKeyLink.isPartner}
        partnerPromotionKey={apiKeyLink.partnerPromotionKey}
        isCopilotPreset={isCopilotProvider}
        isCodexOauthPreset={isCodexOauthProvider}
        usesOAuth={
          templatePreset?.requiresOAuth === true ||
          isCopilotProvider ||
          isCodexOauthProvider
        }
        isCopilotAuthenticated={isCopilotAuthenticated}
        selectedGitHubAccountId={selectedGitHubAccountId}
        onGitHubAccountSelect={setSelectedGitHubAccountId}
        isCodexOauthAuthenticated={isCodexOauthAuthenticated}
        selectedCodexAccountId={selectedCodexAccountId}
        onCodexAccountSelect={setSelectedCodexAccountId}
        codexFastMode={codexFastMode}
        onCodexFastModeChange={setCodexFastMode}
        templateValueEntries={templateValueEntries}
        templateValues={templateValues}
        templatePresetName={templatePreset?.name || ""}
        onTemplateValueChange={handleTemplateValueChange}
        shouldShowSpeedTest={needsEndpointAndKey(category)}
        baseUrl={baseUrl}
        onBaseUrlChange={handleBaseUrlChange}
        isEndpointModalOpen={isEndpointModalOpen}
        onEndpointModalToggle={setIsEndpointModalOpen}
        onCustomEndpointsChange={
          isEditMode ? undefined : endpoints.setDraftCustomEndpoints
        }
        autoSelect={endpoints.endpointAutoSelect}
        onAutoSelectChange={endpoints.setEndpointAutoSelect}
        showEndpointTools
        shouldShowModelSelector={category !== "official"}
        claudeModel={models.claudeModel}
        defaultHaikuModel={models.defaultHaikuModel}
        defaultHaikuModelName={models.defaultHaikuModelName}
        defaultSonnetModel={models.defaultSonnetModel}
        defaultSonnetModelName={models.defaultSonnetModelName}
        defaultOpusModel={models.defaultOpusModel}
        defaultOpusModelName={models.defaultOpusModelName}
        defaultFableModel={models.defaultFableModel}
        defaultFableModelName={models.defaultFableModelName}
        subagentModel={models.subagentModel}
        onModelChange={models.handleModelChange}
        speedTestEndpoints={speedTestEndpoints}
        apiFormat={apiFormat}
        onApiFormatChange={setApiFormat}
        apiKeyField={apiKeyField}
        onApiKeyFieldChange={handleApiKeyFieldChange}
        isFullUrl={isFullUrl}
        onFullUrlChange={setIsFullUrl}
        customUserAgent={overrides.customUserAgent}
        onCustomUserAgentChange={overrides.setCustomUserAgent}
        localProxyHeadersOverride={overrides.headersOverride}
        onLocalProxyHeadersOverrideChange={overrides.setHeadersOverride}
        localProxyBodyOverride={overrides.bodyOverride}
        onLocalProxyBodyOverrideChange={overrides.setBodyOverride}
      />

      <CommonConfigEditor
        value={settingsConfig}
        onChange={ctx.setSettingsConfig}
        useCommonConfig={commonConfig.useCommonConfig}
        onCommonConfigToggle={commonConfig.handleCommonConfigToggle}
        commonConfigSnippet={commonConfig.commonConfigSnippet}
        onCommonConfigSnippetChange={
          commonConfig.handleCommonConfigSnippetChange
        }
        commonConfigError={commonConfig.commonConfigError}
        onEditClick={() => setIsCommonConfigModalOpen(true)}
        isModalOpen={isCommonConfigModalOpen}
        onModalClose={() => setIsCommonConfigModalOpen(false)}
        onExtract={commonConfig.handleExtract}
        isExtracting={commonConfig.isExtracting}
      />
      {ctx.settingsConfigErrorField}
    </>
  );
}

export const claudeFormAdapter: ShellFormAdapter = {
  kind: "shell",
  Body: ClaudeAdapterBody,
  defaultSettingsConfig: CLAUDE_DEFAULT_CONFIG,
  showPricing: () => true,
  hasCommonConfig: true,
};
