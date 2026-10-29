import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { ProviderPresetSelector } from "@/components/providers/forms/ProviderPresetSelector";
import { BasicFormFields } from "@/components/providers/forms/BasicFormFields";
import {
  useApiKeyLink,
  useCodexOauth,
  useCopilotAuth,
  useOpenclawFormState,
  useProviderCategory,
} from "@/components/providers/forms/hooks";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import { useOpenClawLiveProviderIds } from "@/hooks/useOpenClaw";
import { AdditiveProviderKeyField } from "../AdditiveProviderKeyField";
import {
  additiveExistingKeys,
  additiveManagedAuth,
  additiveProviderKeyError,
  managedAuthLoginError,
  providerKeyLockState,
} from "../additiveForm";
import {
  activePresetFrom,
  presetBaseFormValues,
  type ActivePreset,
} from "../basePreset";
import { CommonConfigNotice } from "../CommonConfigNotice";
import {
  corePayload,
  isPricingConfigInvalid,
  usePresetCategoryLabels,
} from "../coreForm";
import { ProviderFormShell, useProviderForm } from "../ProviderFormShell";
import type { ProviderFormProps } from "../types";
import { useSaveGate } from "../useSaveGate";
import { OpenClawFields } from "./Fields";
import { openclawFormFromProvider } from "./fromProvider";
import { OPENCLAW_PRESETS, OPENCLAW_PRESET_ENTRIES } from "./presets";
import {
  toOpenClawProviderMeta,
  toOpenClawSettingsConfig,
  toOpenClawSuggestedDefaults,
} from "./toSettingsConfig";

/** OpenClaw 选中的预设还带着建议的默认模型，提交时一并带上 */
type OpenClawActivePreset = ActivePreset & {
  suggestedDefaults?: OpenClawSuggestedDefaults;
};

/** OpenClaw 供应商编辑器（以 providerKey 为主键写入 live 配置） */
export function OpenClawProviderForm({
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
    () => openclawFormFromProvider(initialData),
    [initialData],
  );
  const presetEntries = OPENCLAW_PRESET_ENTRIES;
  const presetCategoryLabels = usePresetCategoryLabels();

  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    initialData ? null : "custom",
  );
  const [activePreset, setActivePreset] = useState<OpenClawActivePreset | null>(
    null,
  );

  const { category } = useProviderCategory({
    appId: "openclaw",
    selectedPresetId,
    isEditMode,
    initialCategory: initialData?.category,
    presets: OPENCLAW_PRESETS,
  });

  useEffect(() => {
    setSelectedPresetId(initialData ? null : "custom");
    setActivePreset(null);
  }, [initialData]);

  const defaultValues = initial.defaultValues;
  const form = useProviderForm("openclaw", defaultValues);
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

  // Copilot / Codex OAuth 认证状态（导入的托管账号供应商）
  const { isAuthenticated: isCopilotAuthenticated } = useCopilotAuth();
  const { isAuthenticated: isCodexOauthAuthenticated } = useCodexOauth();

  const openclawForm = useOpenclawFormState({
    initialData,
    appId: "openclaw",
    providerId,
    onSettingsConfigChange: (config) => form.setValue("settingsConfig", config),
    getSettingsConfig: () => form.getValues("settingsConfig"),
  });
  const {
    data: openclawLiveProviderIds = [],
    isLoading: isOpenclawLiveProviderIdsLoading,
  } = useOpenClawLiveProviderIds(true);

  const existingKeys = useMemo(
    () =>
      additiveExistingKeys(
        providerId,
        openclawForm.existingOpenclawKeys,
        openclawLiveProviderIds,
      ),
    [openclawForm.existingOpenclawKeys, openclawLiveProviderIds, providerId],
  );
  const { isLocked, isLockStateLoading } = providerKeyLockState({
    isEditMode,
    providerId,
    liveProviderIds: openclawLiveProviderIds,
    isLiveProviderIdsLoading: isOpenclawLiveProviderIdsLoading,
  });

  const managedAuth = additiveManagedAuth(initialData, initial);

  const handleSubmit = async (values: ProviderFormData) => {
    // 软性问题（业务约束，用户可选择仍要保存）
    const issues: string[] = [];

    if (!values.name.trim()) {
      issues.push(t("providerForm.fillSupplierName"));
    }

    if (isPricingConfigInvalid(initial.pricing)) {
      gate.block(t("settings.globalProxy.defaultCostMultiplierInvalid"));
      return;
    }

    const keyError = additiveProviderKeyError(
      {
        key: openclawForm.openclawProviderKey,
        i18nPrefix: "openclaw",
        isLocked,
        isLockStateLoading,
        existingKeys,
      },
      t,
    );
    if (keyError) {
      gate.block(keyError);
      return;
    }

    const loginError = managedAuthLoginError(
      managedAuth,
      { isCopilotAuthenticated, isCodexOauthAuthenticated },
      t,
    );
    if (loginError) {
      gate.block(loginError);
      return;
    }

    if (issues.length > 0) {
      // 弹确认框让用户决定是否仍要保存
      gate.requestConfirm(issues, () => performSubmit(values));
      return;
    }

    await performSubmit(values);
  };

  const performSubmit = async (values: ProviderFormData) => {
    const payload = corePayload(
      values,
      toOpenClawSettingsConfig(values),
      activePreset,
    );
    payload.providerKey = openclawForm.openclawProviderKey;
    // 预设建议的模型引用改成实际提交的 providerKey
    const suggestedDefaults = toOpenClawSuggestedDefaults(
      activePreset?.suggestedDefaults,
      payload.providerKey,
    );
    if (suggestedDefaults) {
      payload.suggestedDefaults = suggestedDefaults;
    }
    payload.meta = toOpenClawProviderMeta(initialData, initial);
    await onSubmit(payload);
  };

  const { shouldShowApiKeyLink, websiteUrl, isPartner, partnerPromotionKey } =
    useApiKeyLink({
      appId: "openclaw",
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
      // 自定义模式：重置为空配置
      openclawForm.resetOpenclawState();
      return;
    }

    const entry = presetEntries.find((item) => item.id === value);
    if (!entry) {
      return;
    }

    setActivePreset({
      ...activePresetFrom(value, entry.preset),
      suggestedDefaults: entry.preset.suggestedDefaults,
    });
    const config = entry.preset.settingsConfig;
    openclawForm.resetOpenclawState(config);
    form.reset(
      presetBaseFormValues(entry.preset, JSON.stringify(config, null, 2), t),
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
            <AdditiveProviderKeyField
              inputId="openclaw-key"
              i18nPrefix="openclaw"
              value={openclawForm.openclawProviderKey}
              onChange={openclawForm.setOpenclawProviderKey}
              isLocked={isLocked}
              isLockStateLoading={isLockStateLoading}
              existingKeys={existingKeys}
            />
          }
        />

        <OpenClawFields
          fields={{
            baseUrl: openclawForm.openclawBaseUrl,
            onBaseUrlChange: openclawForm.handleOpenclawBaseUrlChange,
            apiKey: openclawForm.openclawApiKey,
            onApiKeyChange: openclawForm.handleOpenclawApiKeyChange,
            category,
            shouldShowApiKeyLink,
            websiteUrl,
            isPartner,
            partnerPromotionKey,
            api: openclawForm.openclawApi,
            onApiChange: openclawForm.handleOpenclawApiChange,
            models: openclawForm.openclawModels,
            onModelsChange: openclawForm.handleOpenclawModelsChange,
            userAgent: openclawForm.openclawUserAgent,
            onUserAgentChange: openclawForm.handleOpenclawUserAgentChange,
          }}
        />
      </ProviderFormShell>

      <CommonConfigNotice />
    </>
  );
}
