import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { BasicFormFields } from "@/components/providers/forms/BasicFormFields";
import { ProviderPresetSelector } from "@/components/providers/forms/ProviderPresetSelector";
import { useApiKeyLink } from "@/components/providers/forms/hooks/useApiKeyLink";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ClaudeApiFormat, ProviderCategory, ProviderMeta } from "@/types";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import type { ClaudeDesktopProviderPreset } from "@/config/claudeDesktopProviderPresets";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { providersApi } from "@/lib/api/providers";
import { usePresetCategoryLabels } from "../coreForm";
import { useLazyPresets } from "../lazyPresets";
import { ProviderFormShell, useProviderForm } from "../ProviderFormShell";
import type { ProviderFormInitialData } from "../types";
import { useSaveGate } from "../useSaveGate";
import { ClaudeDesktopFields } from "./Fields";
import {
  claudeDesktopFormFromProvider,
  envString,
  type ClaudeDesktopApiKeyField,
  type ClaudeDesktopMode,
} from "./fromProvider";
import {
  claudeDesktopPresetEntries,
  claudeDesktopPresetLoader,
} from "./presets";
import {
  createRouteRow,
  defaultRouteRows,
  normalizeProxyRows,
  type RouteRow,
} from "./routes";
import {
  toClaudeDesktopOfficialSave,
  toClaudeDesktopSave,
} from "./toSettingsConfig";

export type ClaudeDesktopProviderFormValues = ProviderFormData & {
  presetId?: string;
  presetCategory?: ProviderCategory;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  meta?: ProviderMeta;
  providerKey?: string;
  suggestedDefaults?: OpenClawSuggestedDefaults;
};

export interface ClaudeDesktopProviderFormProps {
  submitLabel: string;
  onSubmit: (values: ClaudeDesktopProviderFormValues) => Promise<void> | void;
  onCancel: () => void;
  onSubmittingChange?: (isSubmitting: boolean) => void;
  initialData?: ProviderFormInitialData;
  showButtons?: boolean;
}

export function ClaudeDesktopProviderForm({
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  initialData,
  showButtons = true,
}: ClaudeDesktopProviderFormProps) {
  const { t } = useTranslation();
  const initial = useMemo(
    () => claudeDesktopFormFromProvider(initialData),
    [initialData],
  );
  const [mode, setMode] = useState<ClaudeDesktopMode>(initial.mode);
  const needsModelMapping = mode === "proxy";
  const [apiFormat, setApiFormat] = useState<ClaudeApiFormat>(
    initial.apiFormat,
  );
  const [baseUrl, setBaseUrl] = useState(initial.baseUrl);
  const [apiKey, setApiKey] = useState(initial.apiKey);
  const [apiKeyField, setApiKeyField] = useState<ClaudeDesktopApiKeyField>(
    initial.apiKeyField,
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
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    "custom",
  );
  const [activePreset, setActivePreset] = useState<{
    id: string;
    category?: ProviderCategory;
    isPartner?: boolean;
    partnerPromotionKey?: string;
    providerType?: string;
    requiresOAuth?: boolean;
  } | null>(null);
  const [routes, setRoutes] = useState<RouteRow[]>(initial.routes);
  const didSeedDefaultRoutes = useRef(initial.hasSavedRoutes);
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [directModelsExpanded, setDirectModelsExpanded] = useState(
    initial.mode === "direct" && initial.hasSavedRoutes,
  );
  const { data: defaultRoutes = [] } = useQuery({
    queryKey: ["claudeDesktopDefaultRoutes"],
    queryFn: () => providersApi.getClaudeDesktopDefaultRoutes(),
  });
  const defaultProxyRouteRows = useMemo(
    () =>
      defaultRouteRows(
        defaultRoutes,
        envString(initialData?.settingsConfig, "ANTHROPIC_MODEL"),
      ),
    [defaultRoutes, initialData?.settingsConfig],
  );

  const defaultValues = initial.defaultValues;
  const form = useProviderForm("claude-desktop", defaultValues);
  const gate = useSaveGate();

  useEffect(() => {
    onSubmittingChange?.(form.formState.isSubmitting || isFetchingModels);
  }, [form.formState.isSubmitting, isFetchingModels, onSubmittingChange]);

  const presets = useLazyPresets(claudeDesktopPresetLoader);
  const presetEntries = useMemo(
    () => (presets ? claudeDesktopPresetEntries(presets) : []),
    [presets],
  );
  const presetCategoryLabels = usePresetCategoryLabels();

  const activeProviderType =
    activePreset?.providerType ?? initialData?.meta?.providerType;
  const isOfficial =
    initialData?.category === "official" ||
    activePreset?.category === "official";
  const usesManagedOAuth =
    activePreset?.requiresOAuth === true ||
    activeProviderType === "github_copilot" ||
    activeProviderType === "codex_oauth";

  // API Key 获取/邀请链接（与 Claude Code 表单同款，见 ClaudeFormFields）
  const apiKeyLinkCategory = activePreset?.category ?? initialData?.category;
  const {
    shouldShowApiKeyLink,
    websiteUrl: apiKeyLinkWebsiteUrl,
    isPartner: apiKeyLinkIsPartner,
    partnerPromotionKey: apiKeyLinkPromotionKey,
  } = useApiKeyLink({
    appId: "claude-desktop",
    category: apiKeyLinkCategory,
    selectedPresetId,
    presetEntries,
    formWebsiteUrl: form.watch("websiteUrl") || "",
  });

  const applyDesktopPreset = (preset: ClaudeDesktopProviderPreset) => {
    form.setValue("name", preset.nameKey ? t(preset.nameKey) : preset.name);
    form.setValue("websiteUrl", preset.websiteUrl);
    form.setValue("notes", "");
    form.setValue("icon", preset.icon ?? "");
    form.setValue("iconColor", preset.iconColor ?? "");

    setBaseUrl(preset.baseUrl);
    setApiKey("");
    setApiKeyField(preset.apiKeyField ?? "ANTHROPIC_AUTH_TOKEN");
    setApiFormat(preset.apiFormat ?? "anthropic");

    didSeedDefaultRoutes.current = true;
    setMode(preset.mode);
    if (preset.mode === "proxy" && preset.modelRoutes) {
      setRoutes(
        normalizeProxyRows(
          preset.modelRoutes.map((r) =>
            createRouteRow({
              route: r.routeId,
              model: r.upstreamModel,
              labelOverride: r.labelOverride ?? "",
              supports1m: r.supports1m,
            }),
          ),
        ),
      );
    } else {
      setRoutes([]);
    }
  };

  const handlePresetChange = (value: string) => {
    setSelectedPresetId(value);

    if (value === "custom") {
      setActivePreset(null);
      form.reset(defaultValues);
      setBaseUrl("");
      setApiKey("");
      setApiKeyField("ANTHROPIC_AUTH_TOKEN");
      setApiFormat("anthropic");
      didSeedDefaultRoutes.current = false;
      setMode("direct");
      setRoutes([]);
      return;
    }

    const entry = presetEntries.find((item) => item.id === value);
    if (!entry) return;

    setActivePreset({
      id: value,
      category: entry.preset.category,
      isPartner: entry.preset.isPartner,
      partnerPromotionKey: entry.preset.partnerPromotionKey,
      providerType: entry.preset.providerType,
      requiresOAuth: entry.preset.requiresOAuth,
    });
    applyDesktopPreset(entry.preset);
  };

  const handleModelMappingChange = (checked: boolean) => {
    setMode(checked ? "proxy" : "direct");
    if (checked) {
      // 切到 proxy：归一化成固定四档；
      // 若当前无路由则以后端默认路由作为来源（保留 Sonnet 默认模型）。
      setRoutes((current) => {
        // 默认路由（默认 1M 声明、ANTHROPIC_MODEL 预填）异步加载完成前，若当前
        // 无路由则保持空数组，交给 seed effect 在加载后回填；不要过早 normalize
        // 成空四档（会把 routes.length 撑到 4、永久挡住 seed）。
        if (current.length === 0 && defaultProxyRouteRows.length === 0) {
          return current;
        }
        const useDefaults =
          current.length === 0 && defaultProxyRouteRows.length > 0;
        if (useDefaults) {
          didSeedDefaultRoutes.current = true;
        }
        return normalizeProxyRows(
          useDefaults ? defaultProxyRouteRows : current,
        );
      });
    }
  };

  useEffect(() => {
    if (
      didSeedDefaultRoutes.current ||
      mode !== "proxy" ||
      routes.length > 0 ||
      defaultProxyRouteRows.length === 0
    ) {
      return;
    }

    didSeedDefaultRoutes.current = true;
    setRoutes(normalizeProxyRows(defaultProxyRouteRows));
  }, [defaultProxyRouteRows, mode, routes.length]);

  const handleFetchModels = async () => {
    if (!baseUrl.trim() || !apiKey.trim()) {
      showFetchModelsError(null, t, {
        hasBaseUrl: Boolean(baseUrl.trim()),
        hasApiKey: Boolean(apiKey.trim()),
      });
      return;
    }

    setIsFetchingModels(true);
    try {
      const models = await fetchModelsForConfig(baseUrl.trim(), apiKey.trim());
      setFetchedModels(models);
      toast.success(
        t("providerForm.fetchModelsSuccess", {
          count: models.length,
        }),
      );
    } catch (error) {
      showFetchModelsError(error, t, {
        hasBaseUrl: Boolean(baseUrl.trim()),
        hasApiKey: Boolean(apiKey.trim()),
      });
    } finally {
      setIsFetchingModels(false);
    }
  };

  const handleSubmit = async (values: ProviderFormData) => {
    if (!values.name.trim()) {
      gate.block(t("providerForm.fillSupplierName"));
      return;
    }
    const trimmed = {
      ...values,
      name: values.name.trim(),
      websiteUrl: values.websiteUrl?.trim() ?? "",
      notes: values.notes?.trim() ?? "",
    };
    if (isOfficial) {
      const { settingsConfig, meta } = toClaudeDesktopOfficialSave(initialData);
      await onSubmit({
        ...trimmed,
        settingsConfig,
        meta,
        presetId: activePreset?.id,
        presetCategory: "official",
      });
      return;
    }
    if (!baseUrl.trim()) {
      gate.block(t("providerForm.fetchModelsNeedEndpoint"));
      return;
    }
    if (!usesManagedOAuth && !apiKey.trim()) {
      gate.block(t("providerForm.fetchModelsNeedApiKey"));
      return;
    }

    const result = toClaudeDesktopSave({
      initialData,
      mode,
      apiFormat,
      baseUrl,
      apiKey,
      apiKeyField,
      usesManagedOAuth,
      routes,
      providerType: activeProviderType,
      githubAccountId: selectedGitHubAccountId,
      codexAccountId: selectedCodexAccountId,
      codexFastMode,
    });
    if (!result.ok) {
      gate.block(t(result.error));
      return;
    }

    await onSubmit({
      ...trimmed,
      settingsConfig: result.settingsConfig,
      meta: result.meta,
      presetId: activePreset?.id,
      presetCategory: activePreset?.category,
      isPartner: activePreset?.isPartner,
      partnerPromotionKey: activePreset?.partnerPromotionKey,
    });
  };

  return (
    <ProviderFormShell
      form={form}
      gate={gate}
      onSubmit={handleSubmit}
      className="space-y-6"
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
          category={activePreset?.category}
        />
      )}

      <BasicFormFields />

      {isOfficial && (
        <div className="rounded-lg border border-border-default bg-muted/20 p-3 text-sm text-muted-foreground">
          {t("claudeDesktop.officialNotice")}
        </div>
      )}

      {!isOfficial && (
        <ClaudeDesktopFields
          usesManagedOAuth={usesManagedOAuth}
          activeProviderType={activeProviderType}
          selectedGitHubAccountId={selectedGitHubAccountId}
          setSelectedGitHubAccountId={setSelectedGitHubAccountId}
          selectedCodexAccountId={selectedCodexAccountId}
          setSelectedCodexAccountId={setSelectedCodexAccountId}
          codexFastMode={codexFastMode}
          setCodexFastMode={setCodexFastMode}
          apiKey={apiKey}
          setApiKey={setApiKey}
          apiKeyLinkCategory={apiKeyLinkCategory}
          shouldShowApiKeyLink={shouldShowApiKeyLink}
          apiKeyLinkWebsiteUrl={apiKeyLinkWebsiteUrl}
          apiKeyLinkIsPartner={apiKeyLinkIsPartner}
          apiKeyLinkPromotionKey={apiKeyLinkPromotionKey}
          baseUrl={baseUrl}
          setBaseUrl={setBaseUrl}
          needsModelMapping={needsModelMapping}
          handleModelMappingChange={handleModelMappingChange}
          apiFormat={apiFormat}
          setApiFormat={setApiFormat}
          routes={routes}
          setRoutes={setRoutes}
          fetchedModels={fetchedModels}
          isFetchingModels={isFetchingModels}
          handleFetchModels={handleFetchModels}
          directModelsExpanded={directModelsExpanded}
          setDirectModelsExpanded={setDirectModelsExpanded}
        />
      )}
    </ProviderFormShell>
  );
}
