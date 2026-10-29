import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { ProviderPresetSelector } from "@/components/providers/forms/ProviderPresetSelector";
import { BasicFormFields } from "@/components/providers/forms/BasicFormFields";
import {
  useApiKeyLink,
  useCodexOauth,
  useCopilotAuth,
  useHermesFormState,
  useProviderCategory,
} from "@/components/providers/forms/hooks";
import { useHermesLiveProviderIds } from "@/hooks/useHermes";
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
import { HermesFields } from "./Fields";
import { hermesFormFromProvider } from "./fromProvider";
import { HERMES_PRESETS, HERMES_PRESET_ENTRIES } from "./presets";
import {
  toHermesProviderMeta,
  toHermesSettingsConfig,
} from "./toSettingsConfig";

/** Hermes 供应商编辑器（以 providerKey 为主键写入 live 配置） */
export function HermesProviderForm({
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
    () => hermesFormFromProvider(initialData),
    [initialData],
  );
  const presetEntries = HERMES_PRESET_ENTRIES;
  const presetCategoryLabels = usePresetCategoryLabels();

  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    initialData ? null : "custom",
  );
  const [activePreset, setActivePreset] = useState<ActivePreset | null>(null);

  const { category } = useProviderCategory({
    appId: "hermes",
    selectedPresetId,
    isEditMode,
    initialCategory: initialData?.category,
    presets: HERMES_PRESETS,
  });

  useEffect(() => {
    setSelectedPresetId(initialData ? null : "custom");
    setActivePreset(null);
  }, [initialData]);

  const defaultValues = initial.defaultValues;
  const form = useProviderForm("hermes", defaultValues);
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

  const hermesForm = useHermesFormState({
    initialData,
    appId: "hermes",
    providerId,
    onSettingsConfigChange: (config) => form.setValue("settingsConfig", config),
    getSettingsConfig: () => form.getValues("settingsConfig"),
  });
  const {
    data: hermesLiveProviderIds = [],
    isLoading: isHermesLiveProviderIdsLoading,
  } = useHermesLiveProviderIds(true);

  const existingKeys = useMemo(
    () =>
      additiveExistingKeys(
        providerId,
        hermesForm.existingHermesKeys,
        hermesLiveProviderIds,
      ),
    [hermesForm.existingHermesKeys, hermesLiveProviderIds, providerId],
  );
  const { isLocked, isLockStateLoading } = providerKeyLockState({
    isEditMode,
    providerId,
    liveProviderIds: hermesLiveProviderIds,
    isLiveProviderIdsLoading: isHermesLiveProviderIdsLoading,
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
        key: hermesForm.hermesProviderKey,
        i18nPrefix: "hermes.form",
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
      toHermesSettingsConfig(values),
      activePreset,
    );
    payload.providerKey = hermesForm.hermesProviderKey;
    payload.meta = toHermesProviderMeta(initialData, initial);
    await onSubmit(payload);
  };

  const { shouldShowApiKeyLink, websiteUrl, isPartner, partnerPromotionKey } =
    useApiKeyLink({
      appId: "hermes",
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
      hermesForm.resetHermesState();
      return;
    }

    const entry = presetEntries.find((item) => item.id === value);
    if (!entry) {
      return;
    }

    setActivePreset(activePresetFrom(value, entry.preset));
    const config = entry.preset.settingsConfig;
    hermesForm.resetHermesState(config);
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
              inputId="hermes-key"
              i18nPrefix="hermes.form"
              value={hermesForm.hermesProviderKey}
              onChange={hermesForm.setHermesProviderKey}
              isLocked={isLocked}
              isLockStateLoading={isLockStateLoading}
              existingKeys={existingKeys}
            />
          }
        />

        <HermesFields
          fields={{
            baseUrl: hermesForm.hermesBaseUrl,
            onBaseUrlChange: hermesForm.handleHermesBaseUrlChange,
            apiKey: hermesForm.hermesApiKey,
            onApiKeyChange: hermesForm.handleHermesApiKeyChange,
            category,
            shouldShowApiKeyLink,
            websiteUrl,
            isPartner,
            partnerPromotionKey,
            apiMode: hermesForm.hermesApiMode,
            onApiModeChange: hermesForm.handleHermesApiModeChange,
            models: hermesForm.hermesModels,
            onModelsChange: hermesForm.handleHermesModelsChange,
            rateLimitDelay: hermesForm.hermesRateLimitDelay,
            onRateLimitDelayChange: hermesForm.handleHermesRateLimitDelayChange,
          }}
        />
      </ProviderFormShell>

      <CommonConfigNotice />
    </>
  );
}
