import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { providersApi } from "@/lib/api";
import { useDarkMode } from "@/hooks/useDarkMode";
import type { ProviderCategory } from "@/types";
import {
  opencodeProviderPresets,
  type OpenCodeProviderPreset,
} from "@/config/opencodeProviderPresets";
import {
  openclawProviderPresets,
  rebaseOpenClawSuggestedDefaults,
  type OpenClawProviderPreset,
  type OpenClawSuggestedDefaults,
} from "@/config/openclawProviderPresets";
import {
  hermesProviderPresets,
  type HermesProviderPreset,
} from "@/config/hermesProviderPresets";
import { OpenCodeFormFields } from "./OpenCodeFormFields";
import { OpenClawFormFields } from "./OpenClawFormFields";
import { HermesFormFields } from "./HermesFormFields";
import JsonEditor from "@/components/JsonEditor";
import { Label } from "@/components/ui/label";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { BasicFormFields } from "./BasicFormFields";
import { OmoFormFields } from "./OmoFormFields";
import { parseOmoOtherFieldsObject } from "@/types/omo";
import {
  useProviderCategory,
  useApiKeyLink,
  useOmoModelSource,
  useOpencodeFormState,
  useOmoDraftState,
  useOpenclawFormState,
  useHermesFormState,
  useCopilotAuth,
  useCodexOauth,
} from "./hooks";
import {
  OPENCODE_DEFAULT_CONFIG,
  OPENCLAW_DEFAULT_CONFIG,
} from "./helpers/opencodeFormUtils";
import { HERMES_DEFAULT_CONFIG } from "./hooks/useHermesFormState";
import { useOpenClawLiveProviderIds } from "@/hooks/useOpenClaw";
import { useHermesLiveProviderIds } from "@/hooks/useHermes";
import { providerKeys } from "@/entities/provider/keys";
import { STALE_TIME } from "@/lib/query";
import {
  AdditiveProviderKeyField,
  PROVIDER_KEY_PATTERN,
  ProviderFormShell,
  SettingsConfigErrorField,
  activePresetFrom,
  presetBaseFormValues,
  providerKeyMessageKeys,
  useProviderForm,
  useSaveGate,
  type ProviderFormProps,
  type ProviderKeyI18nPrefix,
} from "@/features/providers/forms";
import { CommonConfigNotice } from "@/features/providers/forms/CommonConfigNotice";
import {
  coreFormFromProvider,
  corePayload,
  coreProviderMeta,
  formDefaultValues,
  isPricingConfigInvalid,
  usePresetCategoryLabels,
} from "@/features/providers/forms/coreForm";

type PresetEntry = {
  id: string;
  preset:
    | OpenCodeProviderPreset
    | OpenClawProviderPreset
    | HermesProviderPreset;
};

/** 不在这三个应用的界面上编辑的 meta 字段：写 undefined（清掉旧值） */
const ADDITIVE_APP_META = {
  commonConfigEnabled: undefined,
  codexChatReasoning: undefined,
  promptCacheRouting: undefined,
  customUserAgent: undefined,
  localProxyRequestOverrides: undefined,
  apiFormat: undefined,
  apiKeyField: undefined,
  impersonateClaudeCode: undefined,
  maxOutputTokens: undefined,
  isFullUrl: undefined,
} as const;

/**
 * OpenCode / OpenClaw / Hermes 共用的编辑器主体。
 * 外壳（表单、错误汇总、按钮、确认框）由 ProviderFormShell 提供；
 * Claude / Codex / Gemini 已拆到 features/providers/forms/<app>/，
 * 这三个应用由 CCS-192 继续拆分。
 */
export function ProviderFormFull({
  appId,
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
  if (appId !== "opencode" && appId !== "openclaw" && appId !== "hermes") {
    throw new Error(`ProviderFormFull should not receive ${appId}`);
  }

  // 每个应用只比较一次 appId；表单内的分支读取这些布尔值。
  const isOpenCodeApp = appId === "opencode";
  const isOpenClawApp = appId === "openclaw";
  const isHermesApp = appId === "hermes";

  const { t } = useTranslation();
  const isEditMode = Boolean(initialData);
  const isDarkMode = useDarkMode();
  // 端点、计费、托管账号等在这三个应用的界面上不可编辑，保存时沿用已有 meta
  const initial = useMemo(
    () => coreFormFromProvider(initialData),
    [initialData],
  );

  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    initialData ? null : "custom",
  );
  const [activePreset, setActivePreset] = useState<{
    id: string;
    category?: ProviderCategory;
    isPartner?: boolean;
    partnerPromotionKey?: string;
    suggestedDefaults?: OpenClawSuggestedDefaults;
  } | null>(null);

  const appPresets = isOpenCodeApp
    ? opencodeProviderPresets
    : isOpenClawApp
      ? openclawProviderPresets
      : hermesProviderPresets;

  const { category } = useProviderCategory({
    appId,
    selectedPresetId,
    isEditMode,
    initialCategory: initialData?.category,
    presets: appPresets,
  });
  const isOmoCategory = isOpenCodeApp && category === "omo";
  const isOmoSlimCategory = isOpenCodeApp && category === "omo-slim";
  const isAnyOmoCategory = isOmoCategory || isOmoSlimCategory;

  useEffect(() => {
    setSelectedPresetId(initialData ? null : "custom");
    setActivePreset(null);
  }, [appId, initialData]);

  const defaultValues: ProviderFormData = useMemo(
    () =>
      formDefaultValues(
        initialData,
        isOpenCodeApp
          ? OPENCODE_DEFAULT_CONFIG
          : isOpenClawApp
            ? OPENCLAW_DEFAULT_CONFIG
            : HERMES_DEFAULT_CONFIG,
      ),
    [initialData, isOpenClawApp, isOpenCodeApp],
  );

  const form = useProviderForm(appId, defaultValues);
  const { isSubmitting } = form.formState;
  // 阻断错误（内联汇总 + toast）与"仍要保存"确认框的状态
  const gate = useSaveGate();

  const { isConfirmSubmitting } = gate;
  useEffect(() => {
    onSubmittingChange?.(isSubmitting || isConfirmSubmitting);
  }, [isSubmitting, isConfirmSubmitting, onSubmittingChange]);

  // Copilot / Codex OAuth 认证状态（导入的托管账号供应商）
  const { isAuthenticated: isCopilotAuthenticated } = useCopilotAuth();
  const { isAuthenticated: isCodexOauthAuthenticated } = useCodexOauth();

  useEffect(() => {
    form.reset(defaultValues);
  }, [defaultValues, form]);

  const presetCategoryLabels = usePresetCategoryLabels();

  const presetEntries = useMemo<PresetEntry[]>(
    () =>
      appPresets.map((preset, index) => ({
        id: `${appId}-${index}`,
        preset,
      })),
    [appId, appPresets],
  );

  // ── Extracted hooks: OpenCode / OMO / OpenClaw ─────────────────────

  const {
    omoModelOptions,
    omoModelVariantsMap,
    omoPresetMetaMap,
    existingOpencodeKeys,
  } = useOmoModelSource({ isOmoCategory: isAnyOmoCategory, providerId });

  const {
    data: opencodeLiveProviderIds = [],
    isLoading: isOpencodeLiveProviderIdsLoading,
  } = useQuery({
    queryKey: providerKeys.opencodeLiveIds,
    queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
    // 直接读取 OpenCode live 配置文件，可能被外部修改
    staleTime: STALE_TIME.live,
    enabled: isOpenCodeApp && !isAnyOmoCategory,
  });

  const opencodeForm = useOpencodeFormState({
    initialData,
    appId,
    providerId,
    onSettingsConfigChange: (config) => form.setValue("settingsConfig", config),
    getSettingsConfig: () => form.getValues("settingsConfig"),
  });

  const initialOmoSettings =
    isOpenCodeApp &&
    (initialData?.category === "omo" || initialData?.category === "omo-slim")
      ? (initialData.settingsConfig as Record<string, unknown> | undefined)
      : undefined;

  const omoDraft = useOmoDraftState({
    initialOmoSettings,
    isEditMode,
    appId,
    category,
  });

  const openclawForm = useOpenclawFormState({
    initialData,
    appId,
    providerId,
    onSettingsConfigChange: (config) => form.setValue("settingsConfig", config),
    getSettingsConfig: () => form.getValues("settingsConfig"),
  });
  const {
    data: openclawLiveProviderIds = [],
    isLoading: isOpenclawLiveProviderIdsLoading,
  } = useOpenClawLiveProviderIds(isOpenClawApp);

  const hermesForm = useHermesFormState({
    initialData,
    appId,
    providerId,
    onSettingsConfigChange: (config) => form.setValue("settingsConfig", config),
    getSettingsConfig: () => form.getValues("settingsConfig"),
  });
  const {
    data: hermesLiveProviderIds = [],
    isLoading: isHermesLiveProviderIdsLoading,
  } = useHermesLiveProviderIds(isHermesApp);

  const additiveExistingProviderKeys = useMemo(() => {
    if (isOpenCodeApp && !isAnyOmoCategory) {
      return Array.from(
        new Set(
          [...existingOpencodeKeys, ...opencodeLiveProviderIds].filter(
            (key) => key !== providerId,
          ),
        ),
      );
    }

    if (isOpenClawApp) {
      return Array.from(
        new Set(
          [
            ...openclawForm.existingOpenclawKeys,
            ...openclawLiveProviderIds,
          ].filter((key) => key !== providerId),
        ),
      );
    }

    if (isHermesApp) {
      return Array.from(
        new Set(
          [...hermesForm.existingHermesKeys, ...hermesLiveProviderIds].filter(
            (key) => key !== providerId,
          ),
        ),
      );
    }

    return [];
  }, [
    existingOpencodeKeys,
    hermesForm.existingHermesKeys,
    hermesLiveProviderIds,
    isAnyOmoCategory,
    isHermesApp,
    isOpenClawApp,
    isOpenCodeApp,
    openclawForm.existingOpenclawKeys,
    openclawLiveProviderIds,
    opencodeLiveProviderIds,
    providerId,
  ]);

  const isProviderKeyLockStateLoading = useMemo(() => {
    if (!isEditMode) return false;
    if (isOpenCodeApp && !isAnyOmoCategory) {
      return isOpencodeLiveProviderIdsLoading;
    }
    if (isOpenClawApp) {
      return isOpenclawLiveProviderIdsLoading;
    }
    if (isHermesApp) {
      return isHermesLiveProviderIdsLoading;
    }
    return false;
  }, [
    isAnyOmoCategory,
    isEditMode,
    isHermesApp,
    isHermesLiveProviderIdsLoading,
    isOpenClawApp,
    isOpenCodeApp,
    isOpenclawLiveProviderIdsLoading,
    isOpencodeLiveProviderIdsLoading,
  ]);

  const isProviderKeyLocked = useMemo(() => {
    if (!isEditMode || !providerId) return false;
    if (isOpenCodeApp && !isAnyOmoCategory) {
      return opencodeLiveProviderIds.includes(providerId);
    }
    if (isOpenClawApp) {
      return openclawLiveProviderIds.includes(providerId);
    }
    if (isHermesApp) {
      return hermesLiveProviderIds.includes(providerId);
    }
    return false;
  }, [
    hermesLiveProviderIds,
    isAnyOmoCategory,
    isEditMode,
    isHermesApp,
    isOpenClawApp,
    isOpenCodeApp,
    openclawLiveProviderIds,
    opencodeLiveProviderIds,
    providerId,
  ]);

  const additiveProviderKey: {
    inputId: string;
    i18nPrefix: ProviderKeyI18nPrefix;
    value: string;
    onChange: (value: string) => void;
  } | null =
    isOpenCodeApp && !isAnyOmoCategory
      ? {
          inputId: "opencode-key",
          i18nPrefix: "opencode",
          value: opencodeForm.opencodeProviderKey,
          onChange: opencodeForm.setOpencodeProviderKey,
        }
      : isOpenClawApp
        ? {
            inputId: "openclaw-key",
            i18nPrefix: "openclaw",
            value: openclawForm.openclawProviderKey,
            onChange: openclawForm.setOpenclawProviderKey,
          }
        : isHermesApp
          ? {
              inputId: "hermes-key",
              i18nPrefix: "hermes.form",
              value: hermesForm.hermesProviderKey,
              onChange: hermesForm.setHermesProviderKey,
            }
          : null;

  // 托管账号（导入的 Copilot / Codex OAuth 供应商）只能来自已保存的 providerType
  const isCopilotProvider =
    initialData?.meta?.providerType === "github_copilot";
  const isCodexOauthProvider =
    initialData?.meta?.providerType === "codex_oauth";

  const handleSubmit = async (values: ProviderFormData) => {
    // 软性问题（业务约束，用户可选择仍要保存）
    const issues: string[] = [];

    // 供应商名空：A 类
    if (!values.name.trim()) {
      issues.push(t("providerForm.fillSupplierName"));
    }

    if (isPricingConfigInvalid(initial.pricing)) {
      gate.block(t("settings.globalProxy.defaultCostMultiplierInvalid"));
      return;
    }

    // opencode / openclaw / hermes: providerKey 相关
    // providerKey 是主键 ID，空 / 正则不合法 / 重复 / 状态加载中都属于完整性约束，
    // 保留硬拒绝（mutations 层也会 throw，软化只会让错误更晦涩）
    if (additiveProviderKey) {
      const keys = providerKeyMessageKeys(additiveProviderKey.i18nPrefix);
      const key = additiveProviderKey.value;
      const keyError = !key.trim()
        ? t(keys.required)
        : !PROVIDER_KEY_PATTERN.test(key)
          ? t(keys.invalid)
          : isProviderKeyLockStateLoading
            ? t("providerForm.providerKeyStatusLoading")
            : !isProviderKeyLocked && additiveExistingProviderKeys.includes(key)
              ? t(keys.duplicate)
              : null;
      if (keyError) {
        gate.block(keyError);
        return;
      }
    }

    if (
      isOpenCodeApp &&
      !isAnyOmoCategory &&
      Object.keys(opencodeForm.opencodeModels).length === 0
    ) {
      issues.push(t("opencode.modelsRequired"));
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

    // OMO Other Fields JSON：B 类（格式错了保存下去数据就坏了）
    if (
      isOpenCodeApp &&
      isAnyOmoCategory &&
      omoDraft.omoOtherFieldsStr.trim()
    ) {
      try {
        const otherFields = parseOmoOtherFieldsObject(
          omoDraft.omoOtherFieldsStr,
        );
        if (!otherFields) {
          gate.block(
            t("omo.jsonMustBeObject", {
              field: t("omo.otherFields"),
            }),
          );
          return;
        }
      } catch {
        gate.block(t("omo.invalidJson"));
        return;
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
    let settingsConfig: string;

    if (isOpenCodeApp && (category === "omo" || category === "omo-slim")) {
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
        // 格式已在 handleSubmit 前置校验中验证过，此处可以安全解析
        const otherFields = parseOmoOtherFieldsObject(
          omoDraft.omoOtherFieldsStr,
        );
        if (otherFields) {
          omoConfig.otherFields = otherFields;
        }
      }
      settingsConfig = JSON.stringify(omoConfig);
    } else {
      settingsConfig = values.settingsConfig.trim();
    }

    const payload = corePayload(values, settingsConfig, activePreset);

    if (isOpenCodeApp) {
      if (isAnyOmoCategory) {
        if (!isEditMode) {
          const prefix = category === "omo" ? "omo" : "omo-slim";
          payload.providerKey = `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
        }
      } else {
        payload.providerKey = opencodeForm.opencodeProviderKey;
      }
    } else if (isOpenClawApp) {
      payload.providerKey = openclawForm.openclawProviderKey;
    } else if (isHermesApp) {
      payload.providerKey = hermesForm.hermesProviderKey;
    }

    if (isAnyOmoCategory && !payload.presetCategory) {
      payload.presetCategory = category;
    }

    // OpenClaw: align preset model refs with the actual submitted provider key.
    if (activePreset?.suggestedDefaults) {
      payload.suggestedDefaults =
        isOpenClawApp && payload.providerKey
          ? rebaseOpenClawSuggestedDefaults(
              activePreset.suggestedDefaults,
              payload.providerKey,
            )
          : activePreset.suggestedDefaults;
    }

    payload.meta = coreProviderMeta({
      baseMeta: initialData?.meta ? { ...initialData.meta } : undefined,
      endpointAutoSelect: initial.endpointAutoSelect,
      providerType: initialData?.meta?.providerType,
      managedAuth: {
        isCopilotProvider,
        isCodexOauthProvider,
        githubAccountId: initial.githubAccountId,
        codexAccountId: initial.codexAccountId,
        codexFastMode: initial.codexFastMode,
      },
      pricing: initial.pricing,
      app: ADDITIVE_APP_META,
    });

    await onSubmit(payload);
  };

  const { shouldShowApiKeyLink, websiteUrl, isPartner, partnerPromotionKey } =
    useApiKeyLink({
      appId,
      category,
      selectedPresetId,
      presetEntries,
      formWebsiteUrl: form.watch("websiteUrl") || "",
    });

  const handlePresetChange = (value: string) => {
    setSelectedPresetId(value);
    if (value === "custom") {
      setActivePreset(null);
      form.reset(defaultValues);

      if (isOpenCodeApp) {
        opencodeForm.resetOpencodeState();
        omoDraft.resetOmoDraftState();
      }
      // OpenClaw 自定义模式：重置为空配置
      if (isOpenClawApp) {
        openclawForm.resetOpenclawState();
      }
      if (isHermesApp) {
        hermesForm.resetHermesState();
      }
      return;
    }

    const entry = presetEntries.find((item) => item.id === value);
    if (!entry) {
      return;
    }

    setActivePreset(activePresetFrom(value, entry.preset));

    if (isOpenCodeApp) {
      const preset = entry.preset as OpenCodeProviderPreset;
      const config = preset.settingsConfig;

      if (preset.category === "omo" || preset.category === "omo-slim") {
        omoDraft.resetOmoDraftState();
        form.reset(
          presetBaseFormValues(
            preset,
            JSON.stringify({}, null, 2),
            t,
            preset.category === "omo" ? "OMO" : "OMO Slim",
          ),
        );
        return;
      }

      opencodeForm.resetOpencodeState(config);

      form.reset(
        presetBaseFormValues(preset, JSON.stringify(config, null, 2), t),
      );
      return;
    }

    // OpenClaw preset handling
    if (isOpenClawApp) {
      const preset = entry.preset as OpenClawProviderPreset;
      const config = preset.settingsConfig;

      // Update activePreset with suggestedDefaults for OpenClaw
      setActivePreset({
        ...activePresetFrom(value, preset),
        suggestedDefaults: preset.suggestedDefaults,
      });

      openclawForm.resetOpenclawState(config);

      // Update form fields
      form.reset(
        presetBaseFormValues(preset, JSON.stringify(config, null, 2), t),
      );
      return;
    }

    // Hermes preset handling
    const preset = entry.preset as HermesProviderPreset;
    const config = preset.settingsConfig;

    hermesForm.resetHermesState(config);

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

        <BasicFormFields
          beforeNameSlot={
            additiveProviderKey ? (
              <AdditiveProviderKeyField
                inputId={additiveProviderKey.inputId}
                i18nPrefix={additiveProviderKey.i18nPrefix}
                value={additiveProviderKey.value}
                onChange={additiveProviderKey.onChange}
                isLocked={isProviderKeyLocked}
                isLockStateLoading={isProviderKeyLockStateLoading}
                existingKeys={additiveExistingProviderKeys}
              />
            ) : undefined
          }
        />

        {isOpenCodeApp && !isAnyOmoCategory && (
          <OpenCodeFormFields
            npm={opencodeForm.opencodeNpm}
            onNpmChange={opencodeForm.handleOpencodeNpmChange}
            apiKey={opencodeForm.opencodeApiKey}
            onApiKeyChange={opencodeForm.handleOpencodeApiKeyChange}
            category={category}
            shouldShowApiKeyLink={shouldShowApiKeyLink}
            websiteUrl={websiteUrl}
            isPartner={isPartner}
            partnerPromotionKey={partnerPromotionKey}
            baseUrl={opencodeForm.opencodeBaseUrl}
            onBaseUrlChange={opencodeForm.handleOpencodeBaseUrlChange}
            headers={opencodeForm.opencodeHeaders}
            onHeadersChange={opencodeForm.handleOpencodeHeadersChange}
            models={opencodeForm.opencodeModels}
            onModelsChange={opencodeForm.handleOpencodeModelsChange}
            extraOptions={opencodeForm.opencodeExtraOptions}
            onExtraOptionsChange={opencodeForm.handleOpencodeExtraOptionsChange}
          />
        )}

        {isOpenCodeApp && (category === "omo" || category === "omo-slim") && (
          <OmoFormFields
            modelOptions={omoModelOptions}
            modelVariantsMap={omoModelVariantsMap}
            presetMetaMap={omoPresetMetaMap}
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
        )}

        {/* OpenClaw 专属字段 */}
        {isOpenClawApp && (
          <OpenClawFormFields
            baseUrl={openclawForm.openclawBaseUrl}
            onBaseUrlChange={openclawForm.handleOpenclawBaseUrlChange}
            apiKey={openclawForm.openclawApiKey}
            onApiKeyChange={openclawForm.handleOpenclawApiKeyChange}
            category={category}
            shouldShowApiKeyLink={shouldShowApiKeyLink}
            websiteUrl={websiteUrl}
            isPartner={isPartner}
            partnerPromotionKey={partnerPromotionKey}
            api={openclawForm.openclawApi}
            onApiChange={openclawForm.handleOpenclawApiChange}
            models={openclawForm.openclawModels}
            onModelsChange={openclawForm.handleOpenclawModelsChange}
            userAgent={openclawForm.openclawUserAgent}
            onUserAgentChange={openclawForm.handleOpenclawUserAgentChange}
          />
        )}

        {/* Hermes 专属字段 */}
        {isHermesApp && (
          <HermesFormFields
            baseUrl={hermesForm.hermesBaseUrl}
            onBaseUrlChange={hermesForm.handleHermesBaseUrlChange}
            apiKey={hermesForm.hermesApiKey}
            onApiKeyChange={hermesForm.handleHermesApiKeyChange}
            category={category}
            shouldShowApiKeyLink={shouldShowApiKeyLink}
            websiteUrl={websiteUrl}
            isPartner={isPartner}
            partnerPromotionKey={partnerPromotionKey}
            apiMode={hermesForm.hermesApiMode}
            onApiModeChange={hermesForm.handleHermesApiModeChange}
            models={hermesForm.hermesModels}
            onModelsChange={hermesForm.handleHermesModelsChange}
            rateLimitDelay={hermesForm.hermesRateLimitDelay}
            onRateLimitDelayChange={hermesForm.handleHermesRateLimitDelayChange}
          />
        )}

        {isOpenCodeApp && (category === "omo" || category === "omo-slim") ? (
          <div className="space-y-2">
            <Label>{t("provider.configJson")}</Label>
            <JsonEditor
              value={omoDraft.mergedOmoJsonPreview}
              onChange={() => {}}
              rows={14}
              showValidation={false}
              language="json"
              darkMode={isDarkMode}
            />
          </div>
        ) : isOpenCodeApp && category !== "omo" && category !== "omo-slim" ? (
          <>
            <div className="space-y-2">
              <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
              <JsonEditor
                value={form.getValues("settingsConfig")}
                onChange={(config) => form.setValue("settingsConfig", config)}
                placeholder={`{
  "npm": "@ai-sdk/openai-compatible",
  "options": {
    "baseURL": "https://your-api-endpoint.com",
    "apiKey": "your-api-key-here"
  },
  "models": {}
}`}
                rows={14}
                showValidation={true}
                language="json"
                darkMode={isDarkMode}
              />
            </div>
            <SettingsConfigErrorField />
          </>
        ) : (
          <>
            <div className="space-y-2">
              <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
              <JsonEditor
                value={form.getValues("settingsConfig")}
                onChange={(config) => form.setValue("settingsConfig", config)}
                placeholder={
                  isHermesApp
                    ? `{
  "name": "my-provider",
  "base_url": "https://api.example.com/v1",
  "api_key": ""
}`
                    : `{
  "baseUrl": "https://api.example.com/v1",
  "apiKey": "your-api-key-here",
  "api": "openai-completions",
  "models": []
}`
                }
                rows={14}
                showValidation={true}
                language="json"
                darkMode={isDarkMode}
              />
            </div>
            <SettingsConfigErrorField />
          </>
        )}
      </ProviderFormShell>

      <CommonConfigNotice />
    </>
  );
}
