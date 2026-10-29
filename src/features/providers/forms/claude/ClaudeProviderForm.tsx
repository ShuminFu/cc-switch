import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { buildLocalProxyRequestOverrides } from "@/lib/requestOverrides";
import type { ClaudeApiFormat, ClaudeApiKeyField, ProviderMeta } from "@/types";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import { applyTemplateValues } from "@/utils/providerConfigUtils";
import { ProviderPresetSelector } from "@/components/providers/forms/ProviderPresetSelector";
import { BasicFormFields } from "@/components/providers/forms/BasicFormFields";
import { ProviderAdvancedConfig } from "@/components/providers/forms/ProviderAdvancedConfig";
import {
  useApiKeyLink,
  useApiKeyState,
  useBaseUrlState,
  useCodexOauth,
  useCommonConfigSnippet,
  useCopilotAuth,
  useModelState,
  useProviderCategory,
  useSpeedTestEndpoints,
  useTemplateValues,
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
import type { ProviderFormProps } from "../types";
import { useSaveGate } from "../useSaveGate";
import { ClaudeFields } from "./Fields";
import { claudeFormFromProvider } from "./fromProvider";
import { claudePresetEntries, claudePresetLoader } from "./presets";
import {
  claudeAppliesLocalProxyOverrides,
  toClaudeAppMeta,
  toClaudeSettingsConfig,
} from "./toSettingsConfig";

type LocalProxyRequestOverridesBuildResult = ReturnType<
  typeof buildLocalProxyRequestOverrides
>;

/** Claude Code 供应商编辑器 */
export function ClaudeProviderForm({
  providerId,
  submitLabel,
  onSubmit,
  onCancel,
  onUniversalPresetSelect,
  onManageUniversalProviders,
  onSubmittingChange,
  initialData,
  showButtons = true,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const isEditMode = Boolean(initialData);
  const initial = useMemo(
    () => claudeFormFromProvider(initialData),
    [initialData],
  );

  const presets = useLazyPresets(claudePresetLoader);
  const presetEntries = useMemo(
    () => (presets ? claudePresetEntries(presets) : []),
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
  const [selectedGitHubAccountId, setSelectedGitHubAccountId] = useState<
    string | null
  >(initial.githubAccountId);
  const [selectedCodexAccountId, setSelectedCodexAccountId] = useState<
    string | null
  >(initial.codexAccountId);
  const [codexFastMode, setCodexFastMode] = useState<boolean>(
    initial.codexFastMode,
  );
  const [customUserAgent, setCustomUserAgent] = useState<string>(
    initial.customUserAgent,
  );
  const [localProxyHeadersOverride, setLocalProxyHeadersOverride] =
    useState<string>(initial.localProxyHeadersOverride);
  const [localProxyBodyOverride, setLocalProxyBodyOverride] = useState<string>(
    initial.localProxyBodyOverride,
  );
  const [localApiKeyField, setLocalApiKeyField] = useState<ClaudeApiKeyField>(
    initial.apiKeyField,
  );
  const [localApiFormat, setLocalApiFormat] = useState<ClaudeApiFormat>(
    initial.apiFormat,
  );
  const [isCommonConfigModalOpen, setIsCommonConfigModalOpen] = useState(false);

  const { category } = useProviderCategory({
    appId: "claude",
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
    setCustomUserAgent(next.customUserAgent);
    setLocalProxyHeadersOverride(next.localProxyHeadersOverride);
    setLocalProxyBodyOverride(next.localProxyBodyOverride);
  }, [initialData]);

  const defaultValues = initial.defaultValues;
  const form = useProviderForm("claude", defaultValues);
  const { isSubmitting } = form.formState;
  // 阻断错误（内联汇总 + toast）与"仍要保存"确认框的状态
  const gate = useSaveGate();
  const { isConfirmSubmitting } = gate;

  useEffect(() => {
    onSubmittingChange?.(isSubmitting || isConfirmSubmitting);
  }, [isSubmitting, isConfirmSubmitting, onSubmittingChange]);

  useEffect(() => {
    form.reset(defaultValues);
  }, [defaultValues, form]);

  const handleSettingsConfigChange = useCallback(
    (config: string) => {
      form.setValue("settingsConfig", config);
    },
    [form],
  );

  const {
    apiKey,
    handleApiKeyChange,
    showApiKey: shouldShowApiKey,
  } = useApiKeyState({
    initialConfig: form.getValues("settingsConfig"),
    onConfigChange: handleSettingsConfigChange,
    selectedPresetId,
    category,
    appType: "claude",
    apiKeyField: localApiKeyField,
  });

  const { baseUrl, handleClaudeBaseUrlChange } = useBaseUrlState({
    appType: "claude",
    category,
    settingsConfig: form.getValues("settingsConfig"),
    codexConfig: "",
    onSettingsConfigChange: handleSettingsConfigChange,
    onCodexConfigChange: () => {},
  });

  const {
    claudeModel,
    defaultHaikuModel,
    defaultHaikuModelName,
    defaultSonnetModel,
    defaultSonnetModelName,
    defaultOpusModel,
    defaultOpusModelName,
    defaultFableModel,
    defaultFableModelName,
    subagentModel,
    handleModelChange,
  } = useModelState({
    settingsConfig: form.getValues("settingsConfig"),
    onConfigChange: handleSettingsConfigChange,
  });

  const handleApiKeyFieldChange = useCallback(
    (field: ClaudeApiKeyField) => {
      const prev = localApiKeyField;
      setLocalApiKeyField(field);

      // Swap the env key name in settingsConfig
      try {
        const raw = form.getValues("settingsConfig");
        const config = JSON.parse(raw || "{}");
        if (config?.env && prev in config.env) {
          const value = config.env[prev];
          delete config.env[prev];
          config.env[field] = value;
          const updated = JSON.stringify(config, null, 2);
          form.setValue("settingsConfig", updated);
          handleSettingsConfigChange(updated);
        }
      } catch {
        // ignore parse errors during editing
      }
    },
    [localApiKeyField, form, handleSettingsConfigChange],
  );

  // Copilot / Codex OAuth 认证状态
  const { isAuthenticated: isCopilotAuthenticated } = useCopilotAuth();
  const { isAuthenticated: isCodexOauthAuthenticated } = useCodexOauth();

  const {
    templateValues,
    templateValueEntries,
    selectedPreset: templatePreset,
    handleTemplateValueChange,
    validateTemplateValues,
  } = useTemplateValues({
    selectedPresetId,
    presetEntries,
    settingsConfig: form.getValues("settingsConfig"),
    onConfigChange: handleSettingsConfigChange,
  });

  const {
    useCommonConfig,
    commonConfigSnippet,
    commonConfigError,
    handleCommonConfigToggle,
    handleCommonConfigSnippetChange,
    isExtracting,
    handleExtract,
  } = useCommonConfigSnippet({
    settingsConfig: form.getValues("settingsConfig"),
    onConfigChange: handleSettingsConfigChange,
    initialData,
    initialEnabled: initialData?.meta?.commonConfigEnabled,
    selectedPresetId: selectedPresetId ?? undefined,
    enabled: true,
  });

  const {
    shouldShowApiKeyLink,
    websiteUrl: apiKeyLinkWebsiteUrl,
    isPartner,
    partnerPromotionKey,
  } = useApiKeyLink({
    appId: "claude",
    category,
    selectedPresetId,
    presetEntries,
    formWebsiteUrl: form.watch("websiteUrl") || "",
  });

  const speedTestEndpoints = useSpeedTestEndpoints({
    appId: "claude",
    selectedPresetId,
    presetEntries,
    baseUrl,
    codexBaseUrl: "",
    initialData,
  });

  const isCopilotProvider =
    templatePreset?.providerType === "github_copilot" ||
    initialData?.meta?.providerType === "github_copilot" ||
    baseUrl.includes("githubcopilot.com");
  const isCodexOauthProvider =
    templatePreset?.providerType === "codex_oauth" ||
    initialData?.meta?.providerType === "codex_oauth";
  const shouldApplyLocalProxyRequestOverrides =
    claudeAppliesLocalProxyOverrides(category);
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

    // 模板变量未填：A 类（空值）
    if (templateValueEntries.length > 0) {
      const validation = validateTemplateValues();
      if (!validation.isValid && validation.missingField) {
        issues.push(
          t("providerForm.fillParameter", {
            label: validation.missingField.label,
          }),
        );
      }
    }

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

    // 非官方供应商端点 / API Key 空：A 类
    // cloud_provider（如 Bedrock）通过模板变量处理认证，跳过通用校验
    if (category !== "official" && category !== "cloud_provider") {
      if (!isCodexOauthProvider && !baseUrl.trim()) {
        issues.push(t("providerForm.endpointRequired"));
      }
      if (!isCopilotProvider && !isCodexOauthProvider && !apiKey.trim()) {
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
    const payload = corePayload(
      values,
      toClaudeSettingsConfig(values),
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
      // 新建时从预设获取，编辑时从现有数据获取
      providerType:
        templatePreset?.providerType || initialData?.meta?.providerType,
      managedAuth: {
        isCopilotProvider,
        isCodexOauthProvider,
        githubAccountId: selectedGitHubAccountId,
        codexAccountId: selectedCodexAccountId,
        codexFastMode,
      },
      pricing: pricingConfig,
      app: toClaudeAppMeta({
        category,
        commonConfigEnabled: useCommonConfig,
        apiFormat: localApiFormat,
        apiKeyField: localApiKeyField,
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
      return;
    }

    const entry = presetEntries.find((item) => item.id === value);
    if (!entry) {
      return;
    }

    const preset: ProviderPreset = entry.preset;
    setActivePreset(activePresetFrom(value, preset));

    const config = applyTemplateValues(
      preset.settingsConfig,
      preset.templateValues,
    );
    setLocalApiFormat(preset.apiFormat ?? "anthropic");
    setLocalApiKeyField(preset.apiKeyField ?? "ANTHROPIC_AUTH_TOKEN");
    setLocalIsFullUrl(false);

    form.reset(
      presetBaseFormValues(preset, JSON.stringify(config, null, 2), t),
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

        <ClaudeFields
          category={category}
          isEditMode={isEditMode}
          showApiKey={shouldShowApiKey}
          fields={{
            providerId,
            apiKey,
            onApiKeyChange: handleApiKeyChange,
            shouldShowApiKeyLink,
            websiteUrl: apiKeyLinkWebsiteUrl,
            isPartner,
            partnerPromotionKey,
            isCopilotPreset: isCopilotProvider,
            isCodexOauthPreset: isCodexOauthProvider,
            usesOAuth:
              templatePreset?.requiresOAuth === true ||
              isCopilotProvider ||
              isCodexOauthProvider,
            isCopilotAuthenticated,
            selectedGitHubAccountId,
            onGitHubAccountSelect: setSelectedGitHubAccountId,
            isCodexOauthAuthenticated,
            selectedCodexAccountId,
            onCodexAccountSelect: setSelectedCodexAccountId,
            codexFastMode,
            onCodexFastModeChange: setCodexFastMode,
            templateValueEntries,
            templateValues,
            templatePresetName: templatePreset?.name || "",
            onTemplateValueChange: handleTemplateValueChange,
            shouldShowSpeedTest,
            baseUrl,
            onBaseUrlChange: handleClaudeBaseUrlChange,
            isEndpointModalOpen,
            onEndpointModalToggle: setIsEndpointModalOpen,
            onCustomEndpointsChange: isEditMode
              ? undefined
              : setDraftCustomEndpoints,
            autoSelect: endpointAutoSelect,
            onAutoSelectChange: setEndpointAutoSelect,
            showEndpointTools: true,
            shouldShowModelSelector: category !== "official",
            claudeModel,
            defaultHaikuModel,
            defaultHaikuModelName,
            defaultSonnetModel,
            defaultSonnetModelName,
            defaultOpusModel,
            defaultOpusModelName,
            defaultFableModel,
            defaultFableModelName,
            subagentModel,
            onModelChange: handleModelChange,
            speedTestEndpoints,
            apiFormat: localApiFormat,
            onApiFormatChange: setLocalApiFormat,
            apiKeyField: localApiKeyField,
            onApiKeyFieldChange: handleApiKeyFieldChange,
            isFullUrl: localIsFullUrl,
            onFullUrlChange: setLocalIsFullUrl,
            customUserAgent,
            onCustomUserAgentChange: setCustomUserAgent,
            localProxyHeadersOverride,
            onLocalProxyHeadersOverrideChange: setLocalProxyHeadersOverride,
            localProxyBodyOverride,
            onLocalProxyBodyOverrideChange: setLocalProxyBodyOverride,
          }}
          commonConfig={{
            useCommonConfig,
            onCommonConfigToggle: handleCommonConfigToggle,
            commonConfigSnippet,
            onCommonConfigSnippetChange: handleCommonConfigSnippetChange,
            commonConfigError,
            onEditClick: () => setIsCommonConfigModalOpen(true),
            isModalOpen: isCommonConfigModalOpen,
            onModalClose: () => setIsCommonConfigModalOpen(false),
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
