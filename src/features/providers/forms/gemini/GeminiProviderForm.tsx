import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderMeta } from "@/types";
import { ProviderPresetSelector } from "@/components/providers/forms/ProviderPresetSelector";
import { BasicFormFields } from "@/components/providers/forms/BasicFormFields";
import { ProviderAdvancedConfig } from "@/components/providers/forms/ProviderAdvancedConfig";
import {
  useApiKeyLink,
  useApiKeyState,
  useBaseUrlState,
  useCodexOauth,
  useCopilotAuth,
  useGeminiCommonConfig,
  useGeminiConfigState,
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
import { GeminiFields } from "./Fields";
import { geminiFormFromProvider } from "./fromProvider";
import { geminiPresetEntries, geminiPresetLoader } from "./presets";
import { toGeminiAppMeta, toGeminiSettingsConfig } from "./toSettingsConfig";

/** Gemini CLI 供应商编辑器 */
export function GeminiProviderForm({
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
    () => geminiFormFromProvider(initialData),
    [initialData],
  );

  const presets = useLazyPresets(geminiPresetLoader);
  const presetEntries = useMemo(
    () => (presets ? geminiPresetEntries(presets) : []),
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
  const [pricingConfig, setPricingConfig] = useState<PricingConfig>(
    initial.pricing,
  );

  const { category } = useProviderCategory({
    appId: "gemini",
    selectedPresetId,
    isEditMode,
    initialCategory: initialData?.category,
    presets,
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
    setPricingConfig(next.pricing);
  }, [initialData]);

  const defaultValues = initial.defaultValues;
  const form = useProviderForm("gemini", defaultValues);
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

  const { showApiKey: shouldShowApiKey } = useApiKeyState({
    initialConfig: form.getValues("settingsConfig"),
    onConfigChange: handleSettingsConfigChange,
    selectedPresetId,
    category,
    appType: "gemini",
  });

  const { baseUrl } = useBaseUrlState({
    appType: "gemini",
    category,
    settingsConfig: form.getValues("settingsConfig"),
    codexConfig: "",
    onSettingsConfigChange: handleSettingsConfigChange,
    onCodexConfigChange: () => {},
  });

  // Copilot / Codex OAuth 认证状态
  const { isAuthenticated: isCopilotAuthenticated } = useCopilotAuth();
  const { isAuthenticated: isCodexOauthAuthenticated } = useCodexOauth();

  const {
    geminiEnv,
    geminiConfig,
    geminiApiKey,
    geminiBaseUrl,
    geminiModel,
    envError,
    configError: geminiConfigError,
    handleGeminiApiKeyChange: originalHandleGeminiApiKeyChange,
    handleGeminiBaseUrlChange: originalHandleGeminiBaseUrlChange,
    handleGeminiModelChange: originalHandleGeminiModelChange,
    handleGeminiEnvChange,
    handleGeminiConfigChange,
    resetGeminiConfig,
    envStringToObj,
    envObjToString,
  } = useGeminiConfigState({ initialData });

  const updateGeminiEnvField = useCallback(
    (
      key: "GEMINI_API_KEY" | "GOOGLE_GEMINI_BASE_URL" | "GEMINI_MODEL",
      value: string,
    ) => {
      try {
        const config = JSON.parse(form.getValues("settingsConfig") || "{}") as {
          env?: Record<string, unknown>;
        };
        if (!config.env || typeof config.env !== "object") {
          config.env = {};
        }
        config.env[key] = value;
        form.setValue("settingsConfig", JSON.stringify(config, null, 2));
      } catch {
        // ignore parse errors during editing
      }
    },
    [form],
  );

  const handleGeminiApiKeyChange = useCallback(
    (key: string) => {
      originalHandleGeminiApiKeyChange(key);
      updateGeminiEnvField("GEMINI_API_KEY", key.trim());
    },
    [originalHandleGeminiApiKeyChange, updateGeminiEnvField],
  );

  const handleGeminiBaseUrlChange = useCallback(
    (url: string) => {
      originalHandleGeminiBaseUrlChange(url);
      updateGeminiEnvField(
        "GOOGLE_GEMINI_BASE_URL",
        url.trim().replace(/\/+$/, ""),
      );
    },
    [originalHandleGeminiBaseUrlChange, updateGeminiEnvField],
  );

  const handleGeminiModelChange = useCallback(
    (model: string) => {
      originalHandleGeminiModelChange(model);
      updateGeminiEnvField("GEMINI_MODEL", model.trim());
    },
    [originalHandleGeminiModelChange, updateGeminiEnvField],
  );

  const {
    useCommonConfig,
    commonConfigSnippet,
    commonConfigError,
    handleCommonConfigToggle,
    handleCommonConfigSnippetChange,
    isExtracting,
    handleExtract,
    clearCommonConfigError,
  } = useGeminiCommonConfig({
    envValue: geminiEnv,
    onEnvChange: handleGeminiEnvChange,
    envStringToObj,
    envObjToString,
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
    appId: "gemini",
    category,
    selectedPresetId,
    presetEntries,
    formWebsiteUrl: form.watch("websiteUrl") || "",
  });

  const speedTestEndpoints = useSpeedTestEndpoints({
    appId: "gemini",
    selectedPresetId,
    presetEntries,
    baseUrl,
    codexBaseUrl: "",
    initialData,
  });

  // Gemini 没有模板预设：托管账号来自已保存的 providerType 或端点地址
  const isCopilotProvider =
    initialData?.meta?.providerType === "github_copilot" ||
    baseUrl.includes("githubcopilot.com");
  const isCodexOauthProvider =
    initialData?.meta?.providerType === "codex_oauth";
  const shouldShowSpeedTest =
    category !== "official" && category !== "cloud_provider";

  const handleSubmit = async (values: ProviderFormData) => {
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

    // 扩展配置 JSON：B 类（解析失败时必须阻止保存，不能回退到初始配置）
    if (parseJsonObjectForSave(geminiConfig) === null) {
      gate.block(t("providerForm.geminiConfigInvalid"));
      return;
    }

    // 非官方供应商端点 / API Key 空：A 类
    if (category !== "official" && category !== "cloud_provider") {
      if (!geminiBaseUrl.trim()) {
        issues.push(t("providerForm.endpointRequired"));
      }
      if (!geminiApiKey.trim()) {
        issues.push(t("providerForm.apiKeyRequired"));
      }
    }

    if (issues.length > 0) {
      // 弹确认框让用户决定是否仍要保存
      gate.requestConfirm(issues, () => performSubmit(values));
      return;
    }

    await performSubmit(values);
  };

  const performSubmit = async (values: ProviderFormData) => {
    const configObj = parseJsonObjectForSave(geminiConfig);
    if (configObj === null) {
      gate.block(t("providerForm.geminiConfigInvalid"));
      return;
    }

    const payload = corePayload(
      values,
      toGeminiSettingsConfig({ env: geminiEnv, config: configObj }),
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
      app: toGeminiAppMeta({ commonConfigEnabled: useCommonConfig }),
    });

    await onSubmit(payload);
  };

  const handlePresetChange = (value: string) => {
    setSelectedPresetId(value);
    if (value === "custom") {
      setActivePreset(null);
      form.reset(defaultValues);
      resetGeminiConfig({}, {});
      return;
    }

    const entry = presetEntries.find((item) => item.id === value);
    if (!entry) {
      return;
    }

    const preset = entry.preset;
    setActivePreset(activePresetFrom(value, preset));

    const settings = preset.settingsConfig as {
      env?: Record<string, unknown>;
      config?: Record<string, unknown>;
    };
    resetGeminiConfig(settings?.env ?? {}, settings?.config ?? {});

    form.reset(
      presetBaseFormValues(
        preset,
        JSON.stringify(preset.settingsConfig, null, 2),
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

        <GeminiFields
          category={category}
          isEditMode={isEditMode}
          showApiKey={shouldShowApiKey}
          fields={{
            providerId,
            apiKey: geminiApiKey,
            onApiKeyChange: handleGeminiApiKeyChange,
            shouldShowApiKeyLink,
            websiteUrl: apiKeyLinkWebsiteUrl,
            isPartner,
            partnerPromotionKey,
            shouldShowSpeedTest,
            baseUrl: geminiBaseUrl,
            onBaseUrlChange: handleGeminiBaseUrlChange,
            isEndpointModalOpen,
            onEndpointModalToggle: setIsEndpointModalOpen,
            onCustomEndpointsChange: setDraftCustomEndpoints,
            autoSelect: endpointAutoSelect,
            onAutoSelectChange: setEndpointAutoSelect,
            shouldShowModelField: true,
            model: geminiModel,
            onModelChange: handleGeminiModelChange,
            speedTestEndpoints,
          }}
          configEditor={{
            envValue: geminiEnv,
            configValue: geminiConfig,
            onEnvChange: handleGeminiEnvChange,
            onConfigChange: handleGeminiConfigChange,
            useCommonConfig,
            onCommonConfigToggle: handleCommonConfigToggle,
            commonConfigSnippet,
            onCommonConfigSnippetChange: handleCommonConfigSnippetChange,
            onCommonConfigErrorClear: clearCommonConfigError,
            commonConfigError,
            envError,
            configError: geminiConfigError,
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
