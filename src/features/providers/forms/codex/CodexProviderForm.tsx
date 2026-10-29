import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { buildLocalProxyRequestOverrides } from "@/lib/requestOverrides";
import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexChatReasoning,
  PromptCacheRoutingMode,
  ProviderMeta,
} from "@/types";
import {
  codexApiFormatFromWireApi,
  extractCodexWireApi,
  setCodexWireApi,
} from "@/utils/providerConfigUtils";
import { getCodexCustomTemplate } from "@/config/codexTemplates";
import { ProviderPresetSelector } from "@/components/providers/forms/ProviderPresetSelector";
import { BasicFormFields } from "@/components/providers/forms/BasicFormFields";
import { ProviderAdvancedConfig } from "@/components/providers/forms/ProviderAdvancedConfig";
import {
  useApiKeyLink,
  useCodexCommonConfig,
  useCodexConfigState,
  useCodexOauth,
  useCodexTomlValidation,
  useCopilotAuth,
  useProviderCategory,
  useSpeedTestEndpoints,
} from "@/components/providers/forms/hooks";
import {
  activePresetFrom,
  presetBaseFormValues,
  type ActivePreset,
} from "../basePreset";
import { CommonConfigNotice } from "../CommonConfigNotice";
import {
  coreFormFromProvider,
  corePayload,
  coreProviderMeta,
  draftEndpointsMeta,
  isPricingConfigInvalid,
  usePresetCategoryLabels,
  type PricingConfig,
} from "../coreForm";
import { useLazyPresets } from "../lazyPresets";
import { ProviderFormShell, useProviderForm } from "../ProviderFormShell";
import { parseJsonObjectForSave } from "../saveHelpers";
import type { ProviderFormProps } from "../types";
import { useSaveGate } from "../useSaveGate";
import { CodexFields } from "./Fields";
import { codexFormFromProvider } from "./fromProvider";
import { codexPresetEntries, codexPresetLoader } from "./presets";
import {
  codexAppliesLocalProxyOverrides,
  toCodexAppMeta,
  toCodexSettingsConfig,
} from "./toSettingsConfig";

type LocalProxyRequestOverridesBuildResult = ReturnType<
  typeof buildLocalProxyRequestOverrides
>;

/** Codex 供应商编辑器 */
export function CodexProviderForm({
  providerId,
  submitLabel,
  onSubmit,
  onCancel,
  onUniversalPresetSelect,
  onManageUniversalProviders,
  onSubmittingChange,
  initialData,
  showButtons = true,
  isProxyTakeover = false,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const isEditMode = Boolean(initialData);
  const initial = useMemo(
    () => codexFormFromProvider(initialData),
    [initialData],
  );

  const presets = useLazyPresets(codexPresetLoader);
  const presetEntries = useMemo(
    () => (presets ? codexPresetEntries(presets) : []),
    [presets],
  );
  const presetCategoryLabels = usePresetCategoryLabels();

  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    initialData ? null : "custom",
  );
  const [activePreset, setActivePreset] = useState<ActivePreset | null>(null);
  const [isEndpointModalOpen, setIsEndpointModalOpen] = useState(false);
  const [draftCustomEndpoints, setDraftCustomEndpoints] = useState<string[]>(
    [],
  );
  const [endpointAutoSelect, setEndpointAutoSelect] = useState<boolean>(
    initial.endpointAutoSelect,
  );
  const [localIsFullUrl, setLocalIsFullUrl] = useState<boolean>(
    initial.isFullUrl,
  );
  const [pricingConfig, setPricingConfig] = useState<PricingConfig>(
    initial.pricing,
  );
  const [codexChatReasoning, setCodexChatReasoning] =
    useState<CodexChatReasoning>(initial.codexChatReasoning);
  const [promptCacheRouting, setPromptCacheRouting] =
    useState<PromptCacheRoutingMode>(initial.promptCacheRouting);
  const [customUserAgent, setCustomUserAgent] = useState<string>(
    initial.customUserAgent,
  );
  const [localProxyHeadersOverride, setLocalProxyHeadersOverride] =
    useState<string>(initial.localProxyHeadersOverride);
  const [localProxyBodyOverride, setLocalProxyBodyOverride] = useState<string>(
    initial.localProxyBodyOverride,
  );
  const [localCodexApiFormat, setLocalCodexApiFormat] =
    useState<CodexApiFormat>(initial.apiFormat);
  const [localCodexAnthropicAuthField, setLocalCodexAnthropicAuthField] =
    useState<ClaudeApiKeyField>(initial.anthropicAuthField);
  const [localCodexImpersonateClaudeCode, setLocalCodexImpersonateClaudeCode] =
    useState<boolean>(initial.impersonateClaudeCode);
  const [localCodexMaxOutputTokens, setLocalCodexMaxOutputTokens] =
    useState<string>(initial.maxOutputTokens);

  const { category } = useProviderCategory({
    appId: "codex",
    selectedPresetId,
    isEditMode,
    initialCategory: initialData?.category,
    presets,
    officialFallback: true,
  });

  // 切换到另一个供应商（initialData 变化）时重置编辑状态
  useEffect(() => {
    const next = coreFormFromProvider(initialData);
    setSelectedPresetId(initialData ? null : "custom");
    setActivePreset(null);
    if (!initialData) {
      setDraftCustomEndpoints([]);
    }
    setEndpointAutoSelect(next.endpointAutoSelect);
    setLocalIsFullUrl(initialData?.meta?.isFullUrl ?? false);
    setPricingConfig(next.pricing);
    setCodexChatReasoning(initialData?.meta?.codexChatReasoning ?? {});
    setPromptCacheRouting(initialData?.meta?.promptCacheRouting ?? "auto");
    setCustomUserAgent(next.customUserAgent);
    setLocalProxyHeadersOverride(next.localProxyHeadersOverride);
    setLocalProxyBodyOverride(next.localProxyBodyOverride);
  }, [initialData]);

  const defaultValues = initial.defaultValues;
  const form = useProviderForm("codex", defaultValues);
  const { isSubmitting } = form.formState;
  // 阻断错误（内联汇总 + toast）与"仍要保存"确认框的状态
  const gate = useSaveGate();
  const { isConfirmSubmitting } = gate;

  useEffect(() => {
    onSubmittingChange?.(isSubmitting || isConfirmSubmitting);
  }, [isSubmitting, isConfirmSubmitting, onSubmittingChange]);

  // Copilot / Codex OAuth 认证状态（ChatGPT Plus/Pro 反代）
  const { isAuthenticated: isCopilotAuthenticated } = useCopilotAuth();
  const { isAuthenticated: isCodexOauthAuthenticated } = useCodexOauth();

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
    handleCodexConfigChange: originalHandleCodexConfigChange,
    resetCodexConfig,
  } = useCodexConfigState({ initialData });

  const {
    configError: codexConfigError,
    debouncedValidate,
    validateToml: validateCodexToml,
  } = useCodexTomlValidation();

  const handleCodexConfigChange = useCallback(
    (value: string) => {
      originalHandleCodexConfigChange(value);
      debouncedValidate(value);
    },
    [originalHandleCodexConfigChange, debouncedValidate],
  );

  const handleCodexApiFormatChange = useCallback(
    (format: CodexApiFormat) => {
      setLocalCodexApiFormat(format);
      // wire_api is always "responses" for Codex; format controls proxy-layer conversion
      setCodexConfig((prev) => {
        const updated = setCodexWireApi(prev, "responses");
        debouncedValidate(updated);
        return updated;
      });
    },
    [setCodexConfig, debouncedValidate],
  );

  useEffect(() => {
    if (!initialData && selectedPresetId === "custom") {
      const template = getCodexCustomTemplate();
      resetCodexConfig(template.auth, template.config);
      setCodexChatReasoning({});
      setPromptCacheRouting("auto");
    }
  }, [initialData, selectedPresetId, resetCodexConfig]);

  useEffect(() => {
    form.reset(defaultValues);
  }, [defaultValues, form]);

  const {
    useCommonConfig,
    commonConfigSnippet,
    commonConfigError,
    handleCommonConfigToggle,
    handleCommonConfigSnippetChange,
    isExtracting,
    handleExtract,
    clearCommonConfigError,
  } = useCodexCommonConfig({
    codexConfig,
    onConfigChange: handleCodexConfigChange,
    initialData,
    initialEnabled: initialData?.meta?.commonConfigEnabled,
    selectedPresetId: selectedPresetId ?? undefined,
  });

  const {
    shouldShowApiKeyLink,
    websiteUrl: apiKeyLinkWebsiteUrl,
    isPartner,
    partnerPromotionKey,
  } = useApiKeyLink({
    appId: "codex",
    category,
    selectedPresetId,
    presetEntries,
    formWebsiteUrl: form.watch("websiteUrl") || "",
  });

  const speedTestEndpoints = useSpeedTestEndpoints({
    appId: "codex",
    selectedPresetId,
    presetEntries,
    baseUrl: "",
    codexBaseUrl,
    initialData,
  });

  // Codex 没有模板预设：托管账号只能来自已保存的 providerType
  const isCopilotProvider =
    initialData?.meta?.providerType === "github_copilot";
  const isCodexOauthProvider =
    initialData?.meta?.providerType === "codex_oauth";
  const shouldApplyLocalProxyRequestOverrides =
    codexAppliesLocalProxyOverrides(category);
  const shouldShowSpeedTest =
    category !== "official" && category !== "cloud_provider";

  const handleSubmit = async (values: ProviderFormData) => {
    const overridesResult = shouldApplyLocalProxyRequestOverrides
      ? buildLocalProxyRequestOverrides(
          localProxyHeadersOverride,
          localProxyBodyOverride,
        )
      : {};
    if (overridesResult.error) {
      gate.block(
        t("providerForm.localProxyRequestOverridesInvalid", {
          error: overridesResult.error,
        }),
      );
      return;
    }

    // 软性问题（业务约束，用户可选择仍要保存）
    const issues: string[] = [];

    // 供应商名空：A 类
    if (!values.name.trim()) {
      issues.push(t("providerForm.fillSupplierName"));
    }

    if (isPricingConfigInvalid(pricingConfig)) {
      gate.block(t("settings.globalProxy.defaultCostMultiplierInvalid"));
      return;
    }

    // OAuth 未登录：B 类（token 根本不存在，保存了也没法建立）
    if (isCopilotProvider && !isCopilotAuthenticated) {
      gate.block(t("copilot.loginRequired"));
      return;
    }
    if (isCodexOauthProvider && !isCodexOauthAuthenticated) {
      gate.block(t("codexOauth.loginRequired"));
      return;
    }

    // auth.json / config.toml：B 类
    // 解析失败时必须阻止保存，否则只能回退到初始配置，用户的编辑会被静默丢弃
    if (parseJsonObjectForSave(codexAuth) === null) {
      gate.block(t("providerForm.codexAuthInvalid"));
      return;
    }
    // configError 由 debounce 校验产生，提交时同步重新校验，避免读到过期状态
    if (!validateCodexToml(codexConfig ?? "")) {
      gate.block(t("providerForm.codexConfigInvalid"));
      return;
    }

    // 非官方供应商端点 / API Key 空：A 类
    if (category !== "official" && category !== "cloud_provider") {
      if (!codexBaseUrl.trim()) {
        issues.push(t("providerForm.endpointRequired"));
      }
      if (!codexApiKey.trim()) {
        issues.push(t("providerForm.apiKeyRequired"));
      }
    }

    if (issues.length > 0) {
      // 弹确认框让用户决定是否仍要保存
      gate.requestConfirm(issues, () => performSubmit(values, overridesResult));
      return;
    }

    await performSubmit(values, overridesResult);
  };

  const performSubmit = async (
    values: ProviderFormData,
    overridesResult: LocalProxyRequestOverridesBuildResult,
  ) => {
    // 格式已在 handleSubmit 前置校验中验证过；这里仍不回退到初始配置
    const authJson = parseJsonObjectForSave(codexAuth);
    if (authJson === null) {
      gate.block(t("providerForm.codexAuthInvalid"));
      return;
    }

    const payload = corePayload(
      values,
      toCodexSettingsConfig({
        auth: authJson,
        config: codexConfig ?? "",
        catalogModels: codexCatalogModels,
        category,
      }),
      activePreset,
    );

    const endpointsMeta = draftEndpointsMeta({
      isEditMode,
      draftCustomEndpoints,
      initialMeta: initialData?.meta,
      activePreset,
    });
    if (endpointsMeta !== undefined) {
      payload.meta = endpointsMeta;
    }

    const baseMeta: ProviderMeta | undefined =
      payload.meta ?? (initialData?.meta ? { ...initialData.meta } : undefined);

    payload.meta = coreProviderMeta({
      baseMeta,
      endpointAutoSelect,
      providerType: initialData?.meta?.providerType,
      managedAuth: {
        isCopilotProvider,
        isCodexOauthProvider,
        githubAccountId: initial.githubAccountId,
        codexAccountId: initial.codexAccountId,
        codexFastMode: initial.codexFastMode,
      },
      pricing: pricingConfig,
      app: toCodexAppMeta({
        category,
        commonConfigEnabled: useCommonConfig,
        apiFormat: localCodexApiFormat,
        anthropicAuthField: localCodexAnthropicAuthField,
        impersonateClaudeCode: localCodexImpersonateClaudeCode,
        maxOutputTokens: localCodexMaxOutputTokens,
        codexChatReasoning,
        promptCacheRouting,
        isFullUrl: localIsFullUrl,
        customUserAgent,
        localProxyRequestOverrides: overridesResult.overrides,
      }),
    });

    await onSubmit(payload);
  };

  const handlePresetChange = (value: string) => {
    setSelectedPresetId(value);
    if (value === "custom") {
      setActivePreset(null);
      form.reset(defaultValues);

      const template = getCodexCustomTemplate();
      resetCodexConfig(template.auth, template.config);
      setCodexChatReasoning({});
      setPromptCacheRouting("auto");
      setLocalCodexApiFormat(
        codexApiFormatFromWireApi(extractCodexWireApi(template.config)) ??
          "openai_responses",
      );
      return;
    }

    const entry = presetEntries.find((item) => item.id === value);
    if (!entry) {
      return;
    }

    const preset = entry.preset;
    setActivePreset(activePresetFrom(value, preset));

    const auth = preset.auth ?? {};
    const config = preset.config ?? "";

    resetCodexConfig(auth, config, preset.modelCatalog ?? []);
    setCodexChatReasoning(preset.codexChatReasoning ?? {});
    setPromptCacheRouting(preset.promptCacheRouting ?? "auto");
    setLocalCodexApiFormat(
      preset.apiFormat ??
        codexApiFormatFromWireApi(extractCodexWireApi(config)) ??
        "openai_responses",
    );

    form.reset(
      presetBaseFormValues(
        preset,
        JSON.stringify({ auth, config }, null, 2),
        t,
      ),
    );
  };

  return (
    <>
      <ProviderFormShell
        form={form}
        gate={gate}
        onSubmit={handleSubmit}
        className="space-y-6 glass rounded-xl p-6 border border-white/10"
        showButtons={showButtons}
        submitLabel={submitLabel}
        onCancel={onCancel}
      >
        {!initialData && (
          <ProviderPresetSelector
            selectedPresetId={selectedPresetId}
            presetEntries={presetEntries}
            presetCategoryLabels={presetCategoryLabels}
            onPresetChange={handlePresetChange}
            onUniversalPresetSelect={onUniversalPresetSelect}
            onManageUniversalProviders={onManageUniversalProviders}
            category={category}
          />
        )}

        <BasicFormFields />

        <CodexFields
          fields={{
            providerId,
            codexApiKey,
            onApiKeyChange: handleCodexApiKeyChange,
            category,
            shouldShowApiKeyLink,
            websiteUrl: apiKeyLinkWebsiteUrl,
            isPartner,
            partnerPromotionKey,
            shouldShowSpeedTest,
            codexBaseUrl,
            onBaseUrlChange: handleCodexBaseUrlChange,
            isFullUrl: localIsFullUrl,
            onFullUrlChange: setLocalIsFullUrl,
            isEndpointModalOpen,
            onEndpointModalToggle: setIsEndpointModalOpen,
            onCustomEndpointsChange: isEditMode
              ? undefined
              : setDraftCustomEndpoints,
            autoSelect: endpointAutoSelect,
            onAutoSelectChange: setEndpointAutoSelect,
            codexModel,
            onModelChange: handleCodexModelChange,
            apiFormat: localCodexApiFormat,
            onApiFormatChange: handleCodexApiFormatChange,
            anthropicAuthField: localCodexAnthropicAuthField,
            onAnthropicAuthFieldChange: setLocalCodexAnthropicAuthField,
            impersonateClaudeCode: localCodexImpersonateClaudeCode,
            onImpersonateClaudeCodeChange: setLocalCodexImpersonateClaudeCode,
            maxOutputTokens: localCodexMaxOutputTokens,
            onMaxOutputTokensChange: setLocalCodexMaxOutputTokens,
            codexChatReasoning,
            onCodexChatReasoningChange: setCodexChatReasoning,
            promptCacheRouting,
            onPromptCacheRoutingChange: setPromptCacheRouting,
            catalogModels: codexCatalogModels,
            onCatalogModelsChange: setCodexCatalogModels,
            speedTestEndpoints,
            customUserAgent,
            onCustomUserAgentChange: setCustomUserAgent,
            localProxyHeadersOverride,
            onLocalProxyHeadersOverrideChange: setLocalProxyHeadersOverride,
            localProxyBodyOverride,
            onLocalProxyBodyOverrideChange: setLocalProxyBodyOverride,
          }}
          configEditor={{
            authValue: codexAuth,
            configValue: codexConfig,
            showRemoteCompaction: category !== "official",
            isProxyTakeover,
            onAuthChange: setCodexAuth,
            onConfigChange: handleCodexConfigChange,
            useCommonConfig,
            onCommonConfigToggle: handleCommonConfigToggle,
            commonConfigSnippet,
            onCommonConfigSnippetChange: handleCommonConfigSnippetChange,
            onCommonConfigErrorClear: clearCommonConfigError,
            commonConfigError,
            authError: codexAuthError,
            configError: codexConfigError,
            onExtract: handleExtract,
            isExtracting,
          }}
        />

        <ProviderAdvancedConfig
          pricingConfig={pricingConfig}
          onPricingConfigChange={setPricingConfig}
        />
      </ProviderFormShell>

      <CommonConfigNotice />
    </>
  );
}
