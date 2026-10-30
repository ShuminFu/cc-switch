import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Form, FormField, FormItem, FormMessage } from "@/components/ui/form";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import { useDarkMode } from "@/hooks/useDarkMode";
import { isNonNegativeDecimalString } from "@/types/usage";
import {
  CUSTOM_PRESET_ID,
  getPresetEntries,
  type PresetEntry,
} from "@/config/presetRegistry";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { BasicFormFields } from "./BasicFormFields";
import { ProviderAdvancedConfig } from "./ProviderAdvancedConfig";
import { useProviderCategory } from "./hooks";
import { getProviderFormAdapter } from "./adapters";
import type {
  ProviderFormAdapterContext,
  ProviderFormAdapterHandle,
  ProviderFormProps,
  ShellFormAdapter,
} from "./adapters/types";
import {
  buildProviderPayload,
  initialPricingConfig,
  type PricingConfig,
} from "./shell/buildProviderPayload";
import { CommonConfigNotice, SoftIssuesDialog } from "./shell/ShellDialogs";

export type { ProviderFormProps, ProviderFormValues } from "./adapters/types";
export { normalizeCodexCatalogModelsForSave } from "./adapters/codex";

/**
 * Provider form entry point (FE-PROV-17): picks the app's form adapter from
 * the app registry. Standalone adapters render their own form; the others are
 * hosted by the generic shell below, which never branches on the app.
 */
export function ProviderForm(props: ProviderFormProps) {
  const adapter = getProviderFormAdapter(props.appId);
  if (adapter.kind === "standalone") {
    const StandaloneForm = adapter.Form;
    return <StandaloneForm {...props} />;
  }
  return <ProviderFormShell {...props} adapter={adapter} />;
}

let nextBodyKey = 0;
const bodyKeys = new WeakMap<object, number>();
/** Stable per-object key: a new `initialData` remounts the adapter body. */
const bodyKeyOf = (initialData?: object) => {
  if (!initialData) return "new";
  let key = bodyKeys.get(initialData);
  if (key === undefined) {
    key = ++nextBodyKey;
    bodyKeys.set(initialData, key);
  }
  return String(key);
};

function ProviderFormShell({
  adapter,
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
  isProxyTakeover = false,
}: ProviderFormProps & { adapter: ShellFormAdapter }) {
  const { t } = useTranslation();
  const isEditMode = Boolean(initialData);
  const isDarkMode = useDarkMode();
  const handleRef = useRef<ProviderFormAdapterHandle | null>(null);

  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    initialData ? null : CUSTOM_PRESET_ID,
  );
  const [activePreset, setActivePreset] = useState<PresetEntry | null>(null);
  const [pricingConfig, setPricingConfig] = useState<PricingConfig>(() =>
    initialPricingConfig(initialData),
  );
  // Soft validation: business-rule issues the user may save anyway.
  const [softIssues, setSoftIssues] = useState<string[] | null>(null);
  const [pendingValues, setPendingValues] = useState<ProviderFormData | null>(
    null,
  );
  // The confirm path bypasses RHF's isSubmitting, so track it separately.
  const [isConfirmSubmitting, setIsConfirmSubmitting] = useState(false);

  useEffect(() => {
    setSelectedPresetId(initialData ? null : CUSTOM_PRESET_ID);
    setActivePreset(null);
    setPricingConfig(initialPricingConfig(initialData));
  }, [appId, initialData]);

  const { category } = useProviderCategory({
    appId,
    selectedPresetId,
    isEditMode,
    initialCategory: initialData?.category,
  });

  const defaultValues: ProviderFormData = useMemo(
    () => ({
      name: initialData?.name ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      settingsConfig: initialData?.settingsConfig
        ? JSON.stringify(initialData.settingsConfig, null, 2)
        : adapter.defaultSettingsConfig,
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
    }),
    [initialData, adapter],
  );

  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues,
    mode: "onSubmit",
  });
  const { isSubmitting } = form.formState;

  useEffect(() => {
    form.reset(defaultValues);
  }, [defaultValues, form]);

  useEffect(() => {
    onSubmittingChange?.(isSubmitting || isConfirmSubmitting);
  }, [isSubmitting, isConfirmSubmitting, onSubmittingChange]);

  // Subscribe to settingsConfig so JSON-editor edits (form.setValue) reach the
  // adapter's derived API-key / base-URL / model state (FE-PROV-01).
  const settingsConfig =
    useWatch({ control: form.control, name: "settingsConfig" }) ?? "";
  const setSettingsConfig = useCallback(
    (config: string) => form.setValue("settingsConfig", config),
    [form],
  );
  const getSettingsConfig = useCallback(
    () => form.getValues("settingsConfig"),
    [form],
  );

  const presetEntries = useMemo(() => getPresetEntries(appId), [appId]);

  const presetCategoryLabels: Record<string, string> = useMemo(
    () => ({
      official: t("providerForm.categoryOfficial", { defaultValue: "官方" }),
      cn_official: t("providerForm.categoryCnOfficial", {
        defaultValue: "国内官方",
      }),
      aggregator: t("providerForm.categoryAggregation", {
        defaultValue: "聚合服务",
      }),
      third_party: t("providerForm.categoryThirdParty", {
        defaultValue: "第三方",
      }),
      omo: "OMO",
    }),
    [t],
  );

  const ctx: ProviderFormAdapterContext = {
    appId,
    providerId,
    initialData,
    isEditMode,
    isProxyTakeover,
    form,
    settingsConfig,
    setSettingsConfig,
    getSettingsConfig,
    category,
    selectedPresetId,
    presetEntries,
    isDarkMode,
    renderBasicFields: (beforeNameSlot) => (
      <BasicFormFields form={form} beforeNameSlot={beforeNameSlot} />
    ),
    settingsConfigErrorField: (
      <FormField
        control={form.control}
        name="settingsConfig"
        render={() => (
          <FormItem className="space-y-0">
            <FormMessage />
          </FormItem>
        )}
      />
    ),
    handleRef,
  };

  const handlePresetChange = (value: string) => {
    const entry =
      value === CUSTOM_PRESET_ID
        ? null
        : presetEntries.find((item) => item.id === value);
    if (entry === undefined) return;
    setSelectedPresetId(value);
    setActivePreset(entry);

    const base: ProviderFormData = entry
      ? {
          name: entry.preset.nameKey
            ? t(entry.preset.nameKey)
            : entry.preset.name,
          websiteUrl: entry.preset.websiteUrl ?? "",
          settingsConfig: JSON.stringify(
            (entry.preset as { settingsConfig?: unknown }).settingsConfig ?? {},
            null,
            2,
          ),
          icon: entry.preset.icon ?? "",
          iconColor: entry.preset.iconColor ?? "",
        }
      : defaultValues;
    form.reset(handleRef.current?.applyPreset(entry, base) ?? base);
  };

  const performSubmit = async (values: ProviderFormData) => {
    const result = handleRef.current?.serialize(values);
    if (!result) return;
    await onSubmit(
      buildProviderPayload({
        values,
        result,
        activePreset,
        initialData,
        isEditMode,
        pricing: pricingConfig,
      }),
    );
  };

  const handleSubmit = async (values: ProviderFormData) => {
    const validation = handleRef.current?.validate(values) ?? {};
    if (validation.error) {
      toast.error(validation.error);
      return;
    }

    const costMultiplier = pricingConfig.costMultiplier?.trim();
    if (
      pricingConfig.enabled &&
      costMultiplier &&
      !isNonNegativeDecimalString(costMultiplier)
    ) {
      toast.error(
        t("settings.globalProxy.defaultCostMultiplierInvalid", {
          defaultValue: "成本倍率必须为非负数",
        }),
      );
      return;
    }

    const issues: string[] = [];
    if (!values.name.trim()) {
      issues.push(
        t("providerForm.fillSupplierName", {
          defaultValue: "请填写供应商名称",
        }),
      );
    }
    issues.push(...(validation.issues ?? []));

    if (issues.length > 0) {
      setSoftIssues(issues);
      setPendingValues(values);
      return;
    }
    await performSubmit(values);
  };

  const closeSoftIssues = () => {
    setSoftIssues(null);
    setPendingValues(null);
  };

  const confirmSoftIssues = async () => {
    if (isConfirmSubmitting) return;
    if (!pendingValues) {
      closeSoftIssues();
      return;
    }
    setIsConfirmSubmitting(true);
    try {
      await performSubmit(pendingValues);
      closeSoftIssues();
    } catch (error) {
      console.error("[ProviderForm] soft-confirm submit failed:", error);
      // Keep the dialog and pending values so the user can retry.
    } finally {
      setIsConfirmSubmitting(false);
    }
  };

  return (
    <>
      <Form {...form}>
        <form
          id="provider-form"
          onSubmit={form.handleSubmit(handleSubmit)}
          className="space-y-6 glass rounded-xl p-6 border border-white/10"
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

          <adapter.Body key={`${appId}:${bodyKeyOf(initialData)}`} ctx={ctx} />

          {adapter.showPricing(category) && (
            <ProviderAdvancedConfig
              pricingConfig={pricingConfig}
              onPricingConfigChange={setPricingConfig}
            />
          )}

          {showButtons && (
            <div className="flex justify-end gap-2">
              <Button variant="outline" type="button" onClick={onCancel}>
                {t("common.cancel")}
              </Button>
              <Button
                type="submit"
                disabled={isSubmitting || isConfirmSubmitting}
              >
                {submitLabel}
              </Button>
            </div>
          )}
        </form>
      </Form>

      <CommonConfigNotice enabled={adapter.hasCommonConfig} />

      <SoftIssuesDialog
        issues={softIssues}
        onConfirm={confirmSoftIssues}
        onCancel={() => {
          if (!isConfirmSubmitting) closeSoftIssues();
        }}
      />
    </>
  );
}
