import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  PromptCacheRoutingMode,
  ProviderMeta,
} from "@/types";
import type { CodexProviderPreset } from "@/config/codexProviderPresets";
import { getCodexCustomTemplate } from "@/config/codexTemplates";
import {
  codexApiFormatFromWireApi,
  extractCodexModelName,
  extractCodexWireApi,
  setCodexModelName,
  setCodexWireApi,
} from "@/utils/providerConfigUtils";
import CodexConfigEditor from "../CodexConfigEditor";
import { normalizeTomlText } from "@/utils/textNormalization";
import { CodexFormFields } from "../CodexFormFields";
import { CODEX_DEFAULT_CONFIG } from "../helpers/opencodeFormUtils";
import {
  useCodexCommonConfig,
  useCodexConfigState,
  useCodexTomlValidation,
  useSpeedTestEndpoints,
} from "../hooks";
import {
  apiKeyRequiredIssue,
  endpointRequiredIssue,
  isPlainJsonObject,
  needsEndpointAndKey,
  useAdapterApiKeyLink,
  useEndpointDraftState,
  useLocalProxyOverridesState,
  usePublishAdapterHandle,
} from "./shared";
import type { ProviderFormAdapterContext, ShellFormAdapter } from "./types";

export const normalizeCodexCatalogModelsForSave = (
  models: CodexCatalogModel[],
): CodexCatalogModel[] => {
  const seen = new Set<string>();
  const normalized: CodexCatalogModel[] = [];

  for (const item of models) {
    const model = item.model.trim();
    if (!model || seen.has(model)) continue;
    seen.add(model);

    const displayName = item.displayName?.trim();
    const rawContextWindow = String(item.contextWindow ?? "").replace(
      /[^\d]/g,
      "",
    );
    const contextWindow = rawContextWindow
      ? Number.parseInt(rawContextWindow, 10)
      : undefined;

    const inputModalities = item.inputModalities?.filter(
      (m) => typeof m === "string" && m.trim(),
    );

    const baseInstructions = item.baseInstructions?.trim();

    normalized.push({
      model,
      ...(displayName ? { displayName } : {}),
      ...(contextWindow && contextWindow > 0 ? { contextWindow } : {}),
      // Native Responses profile overrides (ignored by the chat/proxy profile).
      ...(typeof item.supportsParallelToolCalls === "boolean"
        ? { supportsParallelToolCalls: item.supportsParallelToolCalls }
        : {}),
      ...(inputModalities && inputModalities.length > 0
        ? { inputModalities }
        : {}),
      ...(baseInstructions ? { baseInstructions } : {}),
    });
  }

  return normalized;
};

const normalizeCodexChatReasoningForSave = (
  value?: CodexChatReasoning,
): CodexChatReasoning | undefined => {
  const supportsEffort = value?.supportsEffort === true;
  const supportsThinking = value?.supportsThinking === true || supportsEffort;
  const hasExplicitConfig = value && Object.keys(value).length > 0;

  if (!supportsThinking && !supportsEffort) {
    return hasExplicitConfig
      ? {
          supportsThinking: false,
          supportsEffort: false,
          thinkingParam: "none",
          effortParam: "none",
          outputFormat: value?.outputFormat ?? "auto",
        }
      : undefined;
  }

  return {
    supportsThinking,
    supportsEffort,
    thinkingParam: supportsThinking
      ? (value?.thinkingParam ?? "thinking")
      : "none",
    effortParam: supportsEffort
      ? (value?.effortParam ?? "reasoning_effort")
      : "none",
    effortValueMode: supportsEffort
      ? (value?.effortValueMode ?? "passthrough")
      : undefined,
    outputFormat: value?.outputFormat ?? "auto",
  };
};

const initialApiFormat = (ctx: ProviderFormAdapterContext): CodexApiFormat => {
  const stored = ctx.initialData?.meta?.apiFormat;
  if (
    stored === "openai_chat" ||
    stored === "anthropic" ||
    stored === "openai_responses"
  ) {
    return stored;
  }
  const toml = ctx.initialData?.settingsConfig?.config;
  return (
    codexApiFormatFromWireApi(
      extractCodexWireApi(typeof toml === "string" ? toml : ""),
    ) ?? "openai_responses"
  );
};

function CodexAdapterBody({ ctx }: { ctx: ProviderFormAdapterContext }) {
  const { t } = useTranslation();
  const { initialData, isEditMode, category, selectedPresetId } = ctx;
  const initialMeta = initialData?.meta;

  const {
    codexAuth,
    codexConfig,
    codexApiKey,
    codexBaseUrl,
    codexModel,
    codexCatalogModels,
    codexAuthError,
    setCodexAuth,
    setCodexConfig,
    setCodexCatalogModels,
    handleCodexApiKeyChange,
    handleCodexBaseUrlChange,
    handleCodexModelChange,
    handleCodexConfigChange: rawHandleCodexConfigChange,
    resetCodexConfig,
  } = useCodexConfigState({ initialData });

  const [apiFormat, setApiFormat] = useState<CodexApiFormat>(() =>
    initialApiFormat(ctx),
  );
  // Auth-field choice for the Anthropic Messages upstream (Bearer by default).
  const [anthropicAuthField, setAnthropicAuthField] =
    useState<ClaudeApiKeyField>(() =>
      initialMeta?.apiKeyField === "ANTHROPIC_API_KEY"
        ? "ANTHROPIC_API_KEY"
        : "ANTHROPIC_AUTH_TOKEN",
    );
  // Emulate the Claude Code client: only when explicitly enabled.
  const [impersonateClaudeCode, setImpersonateClaudeCode] = useState<boolean>(
    () => initialMeta?.impersonateClaudeCode === true,
  );
  // Codex → Anthropic output ceiling override ("" = use the 8192 default),
  // kept as a string so the numeric input can be cleared; parsed on save.
  const [maxOutputTokens, setMaxOutputTokens] = useState<string>(() =>
    typeof initialMeta?.maxOutputTokens === "number" &&
    initialMeta.maxOutputTokens > 0
      ? String(initialMeta.maxOutputTokens)
      : "",
  );
  const [codexChatReasoning, setCodexChatReasoning] =
    useState<CodexChatReasoning>(() => initialMeta?.codexChatReasoning ?? {});
  const [promptCacheRouting, setPromptCacheRouting] =
    useState<PromptCacheRoutingMode>(
      () => initialMeta?.promptCacheRouting ?? "auto",
    );
  const [isFullUrl, setIsFullUrl] = useState<boolean>(
    () => initialMeta?.isFullUrl ?? false,
  );
  const [isEndpointModalOpen, setIsEndpointModalOpen] = useState(false);

  const endpoints = useEndpointDraftState(ctx);
  const overrides = useLocalProxyOverridesState(ctx);

  const {
    configError: codexConfigError,
    smartQuoteFixAvailable,
    debouncedValidate,
    validateToml,
  } = useCodexTomlValidation();

  const handleCodexConfigChange = useCallback(
    (value: string) => {
      rawHandleCodexConfigChange(value);
      debouncedValidate(value);
    },
    [rawHandleCodexConfigChange, debouncedValidate],
  );

  const handleApiFormatChange = useCallback(
    (format: CodexApiFormat) => {
      setApiFormat(format);
      // wire_api is always "responses"; the format drives proxy conversion.
      setCodexConfig((prev) => {
        const updated = setCodexWireApi(prev, "responses");
        debouncedValidate(updated);
        return updated;
      });
    },
    [setCodexConfig, debouncedValidate],
  );

  const applyCustomTemplate = useCallback(() => {
    const template = getCodexCustomTemplate();
    resetCodexConfig(template.auth, template.config);
    setCodexChatReasoning({});
    setPromptCacheRouting("auto");
    return template;
  }, [resetCodexConfig]);

  useEffect(() => {
    if (!initialData && selectedPresetId === "custom") {
      applyCustomTemplate();
    }
  }, [initialData, selectedPresetId, applyCustomTemplate]);

  const commonConfig = useCodexCommonConfig({
    codexConfig,
    onConfigChange: handleCodexConfigChange,
    initialData,
    initialEnabled: initialMeta?.commonConfigEnabled,
    selectedPresetId: selectedPresetId ?? undefined,
  });

  const apiKeyLink = useAdapterApiKeyLink(ctx);
  const speedTestEndpoints = useSpeedTestEndpoints({
    appId: "codex",
    selectedPresetId,
    baseUrl: codexBaseUrl,
    initialData,
    draftCustomEndpoints: isEditMode
      ? undefined
      : endpoints.draftCustomEndpoints,
  });

  // Hard editor errors block saving instead of silently falling back to the
  // stale RHF value (FE-PROV-08).
  const getEditorError = (): string | null => {
    if (codexAuth.trim()) {
      let parsed: unknown;
      try {
        parsed = JSON.parse(codexAuth);
      } catch {
        return t("providerForm.codexAuthInvalid", {
          defaultValue: "auth.json is not valid JSON. Fix it before saving.",
        });
      }
      if (!isPlainJsonObject(parsed)) {
        return t("providerForm.codexAuthNotObject", {
          defaultValue: "auth.json must be a JSON object.",
        });
      }
    }
    if (!validateToml(codexConfig ?? "")) {
      return t("providerForm.codexConfigInvalid", {
        defaultValue: "config.toml has a syntax error. Fix it before saving.",
      });
    }
    return null;
  };

  usePublishAdapterHandle(ctx.handleRef, {
    validate: () => {
      const error = getEditorError() ?? overrides.getError(t);
      if (error) return { error };
      const issues: string[] = [];
      if (needsEndpointAndKey(category)) {
        if (!codexBaseUrl.trim()) issues.push(endpointRequiredIssue(t));
        if (!codexApiKey.trim()) issues.push(apiKeyRequiredIssue(t));
      }
      return { issues };
    },
    serialize: () => {
      const isOfficial = category === "official";
      let authJson: unknown;
      try {
        authJson = codexAuth.trim() ? JSON.parse(codexAuth) : {};
      } catch {
        return null;
      }
      let normalizedConfig =
        !isOfficial && (codexConfig ?? "").trim()
          ? setCodexWireApi(codexConfig ?? "", "responses")
          : (codexConfig ?? "");
      // Model mapping is independent of routing: persist it for every
      // non-official provider; an empty table normalises to [] (not written).
      const catalogModels = isOfficial
        ? []
        : normalizeCodexCatalogModelsForSave(codexCatalogModels);
      // Without an explicit default model fall back to the first mapping row.
      if (
        catalogModels.length > 0 &&
        !extractCodexModelName(normalizedConfig)
      ) {
        normalizedConfig = setCodexModelName(
          normalizedConfig,
          catalogModels[0].model,
        );
      }
      const configObj: {
        auth: unknown;
        config: string;
        modelCatalog?: { models: CodexCatalogModel[] };
      } = { auth: authJson, config: normalizedConfig };
      if (catalogModels.length > 0) {
        configObj.modelCatalog = { models: catalogModels };
      }

      const isChat = !isOfficial && apiFormat === "openai_chat";
      const isAnthropic = !isOfficial && apiFormat === "anthropic";
      const meta: Partial<ProviderMeta> = {
        ...endpoints.toMeta(),
        ...overrides.toMeta(),
        commonConfigEnabled: commonConfig.useCommonConfig,
        codexChatReasoning: isChat
          ? normalizeCodexChatReasoningForSave(codexChatReasoning)
          : undefined,
        promptCacheRouting:
          isChat && promptCacheRouting !== "auto"
            ? promptCacheRouting
            : undefined,
        apiFormat: isOfficial ? undefined : apiFormat,
        apiKeyField:
          isAnthropic && anthropicAuthField !== "ANTHROPIC_AUTH_TOKEN"
            ? anthropicAuthField
            : undefined,
        impersonateClaudeCode:
          isAnthropic && impersonateClaudeCode ? true : undefined,
        maxOutputTokens:
          isAnthropic &&
          maxOutputTokens.trim() !== "" &&
          Number(maxOutputTokens) > 0
            ? Number(maxOutputTokens)
            : undefined,
        isFullUrl: !isOfficial && isFullUrl ? true : undefined,
      };
      return {
        settingsConfig: JSON.stringify(configObj),
        meta,
        customEndpoints: endpoints.pendingCustomEndpoints,
      };
    },
    applyPreset: (entry, formValues) => {
      if (!entry) {
        const template = applyCustomTemplate();
        setApiFormat(
          codexApiFormatFromWireApi(extractCodexWireApi(template.config)) ??
            "openai_responses",
        );
        return formValues;
      }
      const preset = entry.preset as CodexProviderPreset;
      const auth = preset.auth ?? {};
      const config = preset.config ?? "";
      resetCodexConfig(auth, config, preset.modelCatalog ?? []);
      setCodexChatReasoning(preset.codexChatReasoning ?? {});
      setPromptCacheRouting(preset.promptCacheRouting ?? "auto");
      setApiFormat(
        preset.apiFormat ??
          codexApiFormatFromWireApi(extractCodexWireApi(config)) ??
          "openai_responses",
      );
      return {
        ...formValues,
        settingsConfig: JSON.stringify({ auth, config }, null, 2),
      };
    },
  });

  const providerName = ctx.form.watch("name") ?? initialData?.name ?? "";

  return (
    <>
      {ctx.renderBasicFields()}

      <CodexFormFields
        providerId={ctx.providerId}
        codexApiKey={codexApiKey}
        onApiKeyChange={handleCodexApiKeyChange}
        category={category}
        shouldShowApiKeyLink={apiKeyLink.shouldShowApiKeyLink}
        websiteUrl={apiKeyLink.websiteUrl}
        isPartner={apiKeyLink.isPartner}
        partnerPromotionKey={apiKeyLink.partnerPromotionKey}
        shouldShowSpeedTest={needsEndpointAndKey(category)}
        codexBaseUrl={codexBaseUrl}
        onBaseUrlChange={handleCodexBaseUrlChange}
        isFullUrl={isFullUrl}
        onFullUrlChange={setIsFullUrl}
        isEndpointModalOpen={isEndpointModalOpen}
        onEndpointModalToggle={setIsEndpointModalOpen}
        onCustomEndpointsChange={endpoints.setDraftCustomEndpoints}
        pendingCustomEndpoints={endpoints.pendingCustomEndpoints}
        autoSelect={endpoints.endpointAutoSelect}
        onAutoSelectChange={endpoints.setEndpointAutoSelect}
        codexModel={codexModel}
        onModelChange={handleCodexModelChange}
        apiFormat={apiFormat}
        onApiFormatChange={handleApiFormatChange}
        anthropicAuthField={anthropicAuthField}
        onAnthropicAuthFieldChange={setAnthropicAuthField}
        impersonateClaudeCode={impersonateClaudeCode}
        onImpersonateClaudeCodeChange={setImpersonateClaudeCode}
        maxOutputTokens={maxOutputTokens}
        onMaxOutputTokensChange={setMaxOutputTokens}
        codexChatReasoning={codexChatReasoning}
        onCodexChatReasoningChange={setCodexChatReasoning}
        promptCacheRouting={promptCacheRouting}
        onPromptCacheRoutingChange={setPromptCacheRouting}
        catalogModels={codexCatalogModels}
        onCatalogModelsChange={setCodexCatalogModels}
        speedTestEndpoints={speedTestEndpoints}
        customUserAgent={overrides.customUserAgent}
        onCustomUserAgentChange={overrides.setCustomUserAgent}
        localProxyHeadersOverride={overrides.headersOverride}
        onLocalProxyHeadersOverrideChange={overrides.setHeadersOverride}
        localProxyBodyOverride={overrides.bodyOverride}
        onLocalProxyBodyOverrideChange={overrides.setBodyOverride}
      />

      <CodexConfigEditor
        authValue={codexAuth}
        configValue={codexConfig}
        providerName={providerName}
        showRemoteCompaction={category !== "official"}
        isProxyTakeover={ctx.isProxyTakeover}
        onAuthChange={setCodexAuth}
        onConfigChange={handleCodexConfigChange}
        useCommonConfig={commonConfig.useCommonConfig}
        onCommonConfigToggle={commonConfig.handleCommonConfigToggle}
        commonConfigSnippet={commonConfig.commonConfigSnippet}
        onCommonConfigSnippetChange={
          commonConfig.handleCommonConfigSnippetChange
        }
        onCommonConfigErrorClear={commonConfig.clearCommonConfigError}
        commonConfigError={commonConfig.commonConfigError}
        authError={codexAuthError}
        configError={codexConfigError}
        onFixSmartQuotes={
          smartQuoteFixAvailable
            ? () => handleCodexConfigChange(normalizeTomlText(codexConfig))
            : undefined
        }
        onExtract={commonConfig.handleExtract}
        isExtracting={commonConfig.isExtracting}
      />
      {ctx.settingsConfigErrorField}
    </>
  );
}

export const codexFormAdapter: ShellFormAdapter = {
  kind: "shell",
  Body: CodexAdapterBody,
  defaultSettingsConfig: CODEX_DEFAULT_CONFIG,
  showPricing: () => true,
  hasCommonConfig: true,
};
