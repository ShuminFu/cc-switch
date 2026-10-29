import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexChatReasoning,
  PromptCacheRoutingMode,
  ProviderCategory,
} from "@/types";
import { BasicFormFields } from "@/components/providers/forms/BasicFormFields";
import { ProviderPresetSelector } from "@/components/providers/forms/ProviderPresetSelector";
import {
  codexApiFormatFromWireApi,
  extractCodexBaseUrl,
  extractCodexModelName,
  extractCodexWireApi,
} from "@/utils/providerConfigUtils";
import {
  buildGrokBuildConfig,
  parseGrokBuildConfig,
  updateGrokBuildConfig,
  validateGrokBuildConfig,
} from "@/utils/grokBuildConfig";
import { usePresetCategoryLabels } from "../coreForm";
import { useLazyPresets } from "../lazyPresets";
import { ProviderFormShell, useProviderForm } from "../ProviderFormShell";
import type { ProviderFormProps, ProviderFormValues } from "../types";
import { useSaveGate } from "../useSaveGate";
import { GrokBuildFields } from "./Fields";
import {
  grokApiBackendFromApiFormat,
  grokBuildFormFromProvider,
} from "./fromProvider";
import { grokBuildPresetEntries, grokBuildPresetLoader } from "./presets";
import { toGrokBuildSave } from "./toSettingsConfig";

type GrokBuildProviderFormProps = Omit<ProviderFormProps, "appId">;

/** Grok Build 供应商编辑器：结构化字段与原始 config.toml 双向同步 */
export function GrokBuildProviderForm({
  providerId,
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  initialData,
  showButtons = true,
}: GrokBuildProviderFormProps) {
  const { t } = useTranslation();
  const initial = useMemo(
    () => grokBuildFormFromProvider(initialData),
    [initialData],
  );

  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    initialData ? null : "custom",
  );
  const [category, setCategory] = useState<ProviderCategory | undefined>(
    initial.category,
  );
  const [isPartner, setIsPartner] = useState(initial.isPartner);
  const [partnerPromotionKey, setPartnerPromotionKey] = useState<
    string | undefined
  >(initial.partnerPromotionKey);
  const [profile, setProfile] = useState(initial.profile);
  const [upstreamModel, setUpstreamModel] = useState(initial.upstreamModel);
  const [baseUrl, setBaseUrl] = useState(initial.baseUrl);
  const [apiKey, setApiKey] = useState(initial.apiKey);
  const [apiBackend, setApiBackend] = useState(initial.apiBackend);
  const [contextWindow, setContextWindow] = useState(initial.contextWindow);
  const [rawConfig, setRawConfig] = useState(initial.rawConfig);
  const [apiFormat, setApiFormat] = useState<CodexApiFormat>(initial.apiFormat);
  const [anthropicAuthField, setAnthropicAuthField] =
    useState<ClaudeApiKeyField>(initial.anthropicAuthField);
  const [impersonateClaudeCode, setImpersonateClaudeCode] = useState(
    initial.impersonateClaudeCode,
  );
  const [maxOutputTokens, setMaxOutputTokens] = useState(
    initial.maxOutputTokens,
  );
  const [codexChatReasoning, setCodexChatReasoning] =
    useState<CodexChatReasoning>(initial.codexChatReasoning);
  const [promptCacheRouting, setPromptCacheRouting] =
    useState<PromptCacheRoutingMode>(initial.promptCacheRouting);
  const [isFullUrl, setIsFullUrl] = useState(initial.isFullUrl);
  const [customUserAgent, setCustomUserAgent] = useState(
    initial.customUserAgent,
  );
  const [headersOverride, setHeadersOverride] = useState(
    initial.headersOverride,
  );
  const [bodyOverride, setBodyOverride] = useState(initial.bodyOverride);
  const [endpointAutoSelect, setEndpointAutoSelect] = useState(
    initial.endpointAutoSelect,
  );
  const [isEndpointModalOpen, setIsEndpointModalOpen] = useState(false);
  const [presetEndpoints, setPresetEndpoints] = useState<string[]>([]);
  const [draftCustomEndpoints, setDraftCustomEndpoints] = useState<string[]>(
    [],
  );

  const form = useProviderForm("grokbuild", initial.defaultValues);
  const gate = useSaveGate();
  const { isSubmitting } = form.formState;
  const websiteUrl = form.watch("websiteUrl") ?? "";

  useEffect(() => {
    onSubmittingChange?.(isSubmitting);
  }, [isSubmitting, onSubmittingChange]);

  const presets = useLazyPresets(grokBuildPresetLoader);
  const presetEntries = useMemo(
    () => (presets ? grokBuildPresetEntries(presets) : []),
    [presets],
  );
  const presetCategoryLabels = usePresetCategoryLabels();

  const speedTestEndpoints = useMemo(() => {
    const urls = new Set<string>();
    const add = (url?: string) => {
      const normalized = url?.trim().replace(/\/+$/, "");
      if (normalized) urls.add(normalized);
    };
    add(baseUrl);
    presetEndpoints.forEach(add);
    draftCustomEndpoints.forEach(add);
    return Array.from(urls).map((url) => ({ url }));
  }, [baseUrl, draftCustomEndpoints, presetEndpoints]);

  const syncStructuredConfig = (
    overrides: Partial<ReturnType<typeof parseGrokBuildConfig>>,
  ) => {
    const next = {
      model: profile,
      upstreamModel,
      baseUrl,
      name: form.getValues("name") || initial.configName,
      apiKey,
      apiBackend,
      contextWindow: Number.parseInt(contextWindow, 10),
      ...overrides,
    };
    setRawConfig((current) => updateGrokBuildConfig(current, next));
  };

  const handlePresetChange = (presetId: string) => {
    setSelectedPresetId(presetId);
    if (presetId === "custom") {
      setCategory("custom");
      setIsPartner(false);
      setPartnerPromotionKey(undefined);
      setPresetEndpoints([]);
      return;
    }

    const entry = presetEntries.find((candidate) => candidate.id === presetId);
    if (!entry) return;
    const preset = entry.preset;
    const presetName = preset.nameKey ? String(t(preset.nameKey)) : preset.name;
    const presetBaseUrl = extractCodexBaseUrl(preset.config) ?? "";
    const presetModel = extractCodexModelName(preset.config) ?? profile;
    const presetApiFormat =
      preset.apiFormat ??
      codexApiFormatFromWireApi(extractCodexWireApi(preset.config)) ??
      "openai_responses";
    const presetApiKey =
      "auth" in preset && typeof preset.auth?.OPENAI_API_KEY === "string"
        ? preset.auth.OPENAI_API_KEY
        : "";
    const presetApiBackend = grokApiBackendFromApiFormat(presetApiFormat);

    form.setValue("name", presetName);
    form.setValue("websiteUrl", preset.websiteUrl ?? "");
    form.setValue("icon", preset.icon ?? "");
    form.setValue("iconColor", preset.iconColor ?? "");
    setCategory(preset.category ?? "custom");
    setIsPartner(preset.isPartner ?? false);
    setPartnerPromotionKey(preset.partnerPromotionKey);
    setBaseUrl(presetBaseUrl);
    setApiKey(presetApiKey);
    setUpstreamModel(presetModel);
    setApiFormat(presetApiFormat);
    setApiBackend(presetApiBackend);
    setPresetEndpoints(preset.endpointCandidates ?? []);
    setRawConfig(
      buildGrokBuildConfig({
        model: profile,
        upstreamModel: presetModel,
        baseUrl: presetBaseUrl,
        name: presetName,
        apiKey: presetApiKey,
        apiBackend: presetApiBackend,
        contextWindow: Number.parseInt(contextWindow, 10),
      }),
    );
  };

  const handleRawConfigChange = (value: string) => {
    setRawConfig(value);
    if (validateGrokBuildConfig(value)) return;
    const parsed = parseGrokBuildConfig(value, form.getValues("name"));
    setProfile(parsed.model);
    setUpstreamModel(parsed.upstreamModel ?? parsed.model);
    setBaseUrl(parsed.baseUrl);
    setApiKey(parsed.apiKey);
    setApiBackend(parsed.apiBackend);
    setContextWindow(String(parsed.contextWindow));
    if (parsed.name) form.setValue("name", parsed.name);
  };

  const handleSubmit = async (values: ProviderFormData) => {
    const name = values.name.trim();
    const result = toGrokBuildSave({
      name,
      providerId,
      initialMeta: initialData?.meta,
      rawConfig,
      profile,
      upstreamModel,
      baseUrl,
      apiKey,
      apiBackend,
      contextWindow,
      apiFormat,
      anthropicAuthField,
      isFullUrl,
      endpointAutoSelect,
      isPartner,
      partnerPromotionKey,
      impersonateClaudeCode,
      promptCacheRouting,
      codexChatReasoning,
      customUserAgent,
      headersOverride,
      bodyOverride,
      maxOutputTokens,
      draftCustomEndpoints,
    });
    if (!result.ok) {
      const { error } = result;
      gate.block(
        error.key === null
          ? error.error
          : error.key === "grokBuild.invalidToml"
            ? t(error.key, { error: error.error })
            : t(error.key),
      );
      return;
    }

    const payload: ProviderFormValues = {
      ...values,
      name,
      websiteUrl: values.websiteUrl?.trim() ?? "",
      notes: values.notes?.trim() ?? "",
      settingsConfig: result.settingsConfig,
      presetId: selectedPresetId ?? undefined,
      presetCategory: category ?? "custom",
      isPartner,
      meta: result.meta,
    };

    await onSubmit(payload);
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
          category={category}
        />
      )}

      <BasicFormFields />

      <GrokBuildFields
        profile={profile}
        onProfileChange={(value) => {
          setProfile(value);
          syncStructuredConfig({ model: value });
        }}
        apiBackend={apiBackend}
        onApiBackendChange={(value) => {
          setApiBackend(value);
          syncStructuredConfig({ apiBackend: value });
        }}
        contextWindow={contextWindow}
        onContextWindowChange={(value) => {
          setContextWindow(value);
          syncStructuredConfig({
            contextWindow: Number.parseInt(value, 10),
          });
        }}
        codexFields={{
          providerId,
          codexApiKey: apiKey,
          onApiKeyChange: (value) => {
            setApiKey(value);
            syncStructuredConfig({ apiKey: value });
          },
          category,
          shouldShowApiKeyLink: Boolean(websiteUrl),
          websiteUrl,
          isPartner,
          partnerPromotionKey,
          shouldShowSpeedTest: true,
          codexBaseUrl: baseUrl,
          onBaseUrlChange: (value) => {
            setBaseUrl(value);
            syncStructuredConfig({ baseUrl: value });
          },
          isFullUrl,
          onFullUrlChange: setIsFullUrl,
          isEndpointModalOpen,
          onEndpointModalToggle: setIsEndpointModalOpen,
          onCustomEndpointsChange: setDraftCustomEndpoints,
          autoSelect: endpointAutoSelect,
          onAutoSelectChange: setEndpointAutoSelect,
          codexModel: upstreamModel,
          onModelChange: (value) => {
            setUpstreamModel(value);
            syncStructuredConfig({ upstreamModel: value });
          },
          apiFormat,
          onApiFormatChange: (value) => {
            const backend = grokApiBackendFromApiFormat(value);
            setApiFormat(value);
            setApiBackend(backend);
            syncStructuredConfig({ apiBackend: backend });
          },
          anthropicAuthField,
          onAnthropicAuthFieldChange: setAnthropicAuthField,
          impersonateClaudeCode,
          onImpersonateClaudeCodeChange: setImpersonateClaudeCode,
          maxOutputTokens,
          onMaxOutputTokensChange: setMaxOutputTokens,
          codexChatReasoning,
          onCodexChatReasoningChange: setCodexChatReasoning,
          promptCacheRouting,
          onPromptCacheRoutingChange: setPromptCacheRouting,
          speedTestEndpoints,
          customUserAgent,
          onCustomUserAgentChange: setCustomUserAgent,
          localProxyHeadersOverride: headersOverride,
          onLocalProxyHeadersOverrideChange: setHeadersOverride,
          localProxyBodyOverride: bodyOverride,
          onLocalProxyBodyOverrideChange: setBodyOverride,
        }}
        rawConfig={rawConfig}
        onRawConfigChange={handleRawConfigChange}
      />
    </ProviderFormShell>
  );
}
