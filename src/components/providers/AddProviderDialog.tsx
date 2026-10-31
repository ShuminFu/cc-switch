import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Provider, CustomEndpoint, UniversalProvider } from "@/types";
import type { AppId } from "@/lib/api";
import { universalProvidersApi } from "@/lib/api";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { UniversalProviderFormModal } from "@/components/universal/UniversalProviderFormModal";
import { UniversalProviderPanel } from "@/components/universal";
import { claudePresetLoader } from "@/features/providers/forms/claude/presets";
import { codexPresetLoader } from "@/features/providers/forms/codex/presets";
import { geminiPresetLoader } from "@/features/providers/forms/gemini/presets";
import { claudeDesktopPresetLoader } from "@/features/providers/forms/claude-desktop/presets";
import type { PresetLoader } from "@/features/providers/forms/lazyPresets";
import type { BasePreset } from "@/features/providers/forms/basePreset";
import { appCaps } from "@/entities/app/registry";
import { extractProviderCredentials } from "@/entities/provider/settingsConfig";
import { useUnsavedChangesGuard } from "@/features/providers/forms/useUnsavedChangesGuard";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import type { UniversalProviderPreset } from "@/config/universalProviderPresets";

type OfficialSeedFlag =
  | "ensureClaudeDesktopOfficialSeed"
  | "ensureCodexOfficialSeed";

interface PresetSource {
  loader: PresetLoader<readonly BasePreset[]>;
  /** 预设 ID 前缀（`<prefix><index>`） */
  idPrefix: string;
  /** 预设本身携带的请求地址（候选地址之外） */
  presetUrls?: (preset: BasePreset) => Array<string | undefined>;
  /** 选中官方预设时需要后端补齐官方种子供应商的标记 */
  officialSeedFlag?: OfficialSeedFlag;
}

/** 提交时需要回查预设（候选地址 / 官方种子）的应用 */
const PRESET_SOURCES: Partial<Record<AppId, PresetSource>> = {
  claude: { loader: claudePresetLoader, idPrefix: "claude-" },
  codex: {
    loader: codexPresetLoader,
    idPrefix: "codex-",
    officialSeedFlag: "ensureCodexOfficialSeed",
  },
  gemini: { loader: geminiPresetLoader, idPrefix: "gemini-" },
  "claude-desktop": {
    loader: claudeDesktopPresetLoader,
    idPrefix: "claude-desktop-",
    presetUrls: (preset) => [(preset as { baseUrl?: string }).baseUrl],
    officialSeedFlag: "ensureClaudeDesktopOfficialSeed",
  },
};

/** 按预设 ID 查找预设；ID 无效或越界时返回 undefined */
async function resolvePreset(
  source: PresetSource,
  presetId: string,
): Promise<BasePreset | undefined> {
  const presets = await source.loader.load();
  const presetIndex = parseInt(presetId.replace(source.idPrefix, ""));
  if (isNaN(presetIndex) || presetIndex < 0 || presetIndex >= presets.length) {
    return undefined;
  }
  return presets[presetIndex];
}

interface AddProviderDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  appId: AppId;
  onSubmit: (
    provider: Omit<Provider, "id"> & {
      providerKey?: string;
      suggestedDefaults?: OpenClawSuggestedDefaults;
      ensureClaudeDesktopOfficialSeed?: boolean;
      ensureCodexOfficialSeed?: boolean;
    },
  ) => Promise<void> | void;
}

export function AddProviderDialog({
  open,
  onOpenChange,
  appId,
  onSubmit,
}: AddProviderDialogProps) {
  const { t } = useTranslation();
  const showUniversalTab = appCaps(appId).universalProviders;
  const [activeTab, setActiveTab] = useState<"app-specific" | "universal">(
    "app-specific",
  );
  const [universalFormOpen, setUniversalFormOpen] = useState(false);
  const [selectedUniversalPreset, setSelectedUniversalPreset] =
    useState<UniversalProviderPreset | null>(null);
  const [isFormSubmitting, setIsFormSubmitting] = useState(false);
  const closePanel = useCallback(() => onOpenChange(false), [onOpenChange]);
  // 用户主动关闭（Esc/返回/取消）时若有未保存修改先确认；保存成功后直接关闭
  const unsavedGuard = useUnsavedChangesGuard(closePanel);

  const handleUniversalProviderSave = useCallback(
    async (provider: UniversalProvider) => {
      try {
        await universalProvidersApi.upsert(provider);
      } catch (error) {
        console.error(
          "[AddProviderDialog] Failed to save universal provider",
          error,
        );
        toast.error(t("universalProvider.addFailed"));
        return;
      }

      try {
        await universalProvidersApi.sync(provider.id);
        toast.success(t("universalProvider.addedAndSynced"));
      } catch (error) {
        console.error(
          "[AddProviderDialog] Provider saved but sync failed",
          error,
        );
        toast.warning(t("universalProvider.addedButSyncFailed"));
      }

      setUniversalFormOpen(false);
      setSelectedUniversalPreset(null);
      onOpenChange(false);
    },
    [t, onOpenChange],
  );

  const handleUniversalFormClose = useCallback(() => {
    setUniversalFormOpen(false);
    setSelectedUniversalPreset(null);
  }, []);

  const handleSubmit = useCallback(
    async (values: ProviderFormValues) => {
      const parsedConfig = JSON.parse(values.settingsConfig) as Record<
        string,
        unknown
      >;

      // 构造基础提交数据
      const providerData: Omit<Provider, "id"> & {
        providerKey?: string;
        suggestedDefaults?: OpenClawSuggestedDefaults;
        ensureClaudeDesktopOfficialSeed?: boolean;
        ensureCodexOfficialSeed?: boolean;
      } = {
        name: values.name.trim(),
        notes: values.notes?.trim() || undefined,
        websiteUrl: values.websiteUrl?.trim() || undefined,
        settingsConfig: parsedConfig,
        icon: values.icon?.trim() || undefined,
        iconColor: values.iconColor?.trim() || undefined,
        ...(values.presetCategory ? { category: values.presetCategory } : {}),
        ...(values.meta ? { meta: values.meta } : {}),
      };

      const presetSource = PRESET_SOURCES[appId];
      if (presetSource?.officialSeedFlag && values.presetId) {
        const preset = await resolvePreset(presetSource, values.presetId);
        providerData[presetSource.officialSeedFlag] =
          values.presetCategory === "official" &&
          preset?.category === "official";
      }

      // OpenCode/OpenClaw: pass providerKey for ID generation
      if (appCaps(appId).additive && values.providerKey) {
        providerData.providerKey = values.providerKey;
      }

      const hasCustomEndpoints =
        providerData.meta?.custom_endpoints &&
        Object.keys(providerData.meta.custom_endpoints).length > 0;

      if (!hasCustomEndpoints && values.presetCategory !== "omo") {
        const urlSet = new Set<string>();

        const addUrl = (rawUrl?: string) => {
          const url = (rawUrl || "").trim().replace(/\/+$/, "");
          if (url && url.startsWith("http")) {
            urlSet.add(url);
          }
        };

        if (values.presetId && presetSource) {
          const preset = await resolvePreset(presetSource, values.presetId);
          if (preset) {
            preset.endpointCandidates?.forEach(addUrl);
            presetSource.presetUrls?.(preset).forEach(addUrl);
          }
        }

        addUrl(extractProviderCredentials(appId, parsedConfig).baseUrl);

        const urls = Array.from(urlSet);
        if (urls.length > 0) {
          const now = Date.now();
          const customEndpoints: Record<string, CustomEndpoint> = {};
          urls.forEach((url) => {
            customEndpoints[url] = {
              url,
              addedAt: now,
              lastUsed: undefined,
            };
          });

          providerData.meta = {
            ...(providerData.meta ?? {}),
            custom_endpoints: customEndpoints,
          };
        }
      }

      // OpenClaw: pass suggestedDefaults for model registration
      if (appCaps(appId).defaultModel && values.suggestedDefaults) {
        providerData.suggestedDefaults = values.suggestedDefaults;
      }

      await onSubmit(providerData);
      onOpenChange(false);
    },
    [appId, onSubmit, onOpenChange],
  );

  const footer =
    !showUniversalTab || activeTab === "app-specific" ? (
      <>
        <span className="mr-auto min-w-0 text-xs text-muted-foreground truncate">
          {t("provider.addFooterHint")}
        </span>
        <Button
          variant="outline"
          onClick={unsavedGuard.requestClose}
          className="border-border/20 hover:bg-accent hover:text-accent-foreground"
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="submit"
          form="provider-form"
          data-testid="add-provider-submit"
          disabled={isFormSubmitting}
          className="bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Plus className="h-4 w-4 mr-2" />
          {t("common.add")}
        </Button>
      </>
    ) : (
      <>
        <Button
          variant="outline"
          onClick={unsavedGuard.requestClose}
          className="border-border/20 hover:bg-accent hover:text-accent-foreground"
        >
          {t("common.cancel")}
        </Button>
        <Button
          onClick={() => setUniversalFormOpen(true)}
          className="bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Plus className="h-4 w-4 mr-2" />
          {t("universalProvider.add")}
        </Button>
      </>
    );

  return (
    <FullScreenPanel
      isOpen={open}
      title={t("provider.addNewProvider")}
      onClose={unsavedGuard.requestClose}
      footer={footer}
      contentClassName="pt-3"
    >
      {showUniversalTab ? (
        <Tabs
          value={activeTab}
          onValueChange={(v) => setActiveTab(v as "app-specific" | "universal")}
        >
          <TabsList className="grid w-full grid-cols-2 mb-6">
            <TabsTrigger value="app-specific">
              {t(`apps.${appId}`)} {t("provider.tabProvider")}
            </TabsTrigger>
            <TabsTrigger value="universal">
              {t("provider.tabUniversal")}
            </TabsTrigger>
          </TabsList>

          <TabsContent value="app-specific" className="mt-0">
            <ProviderForm
              appId={appId}
              submitLabel={t("common.add")}
              onSubmit={handleSubmit}
              onCancel={unsavedGuard.requestClose}
              onSubmittingChange={setIsFormSubmitting}
              onDirtyChange={unsavedGuard.onDirtyChange}
              showButtons={false}
            />
          </TabsContent>

          <TabsContent value="universal" className="mt-0">
            <UniversalProviderPanel />
          </TabsContent>
        </Tabs>
      ) : (
        // OpenCode/OpenClaw: directly show form without tabs
        <ProviderForm
          appId={appId}
          submitLabel={t("common.add")}
          onSubmit={handleSubmit}
          onCancel={unsavedGuard.requestClose}
          onSubmittingChange={setIsFormSubmitting}
          onDirtyChange={unsavedGuard.onDirtyChange}
          showButtons={false}
        />
      )}

      {showUniversalTab && (
        <UniversalProviderFormModal
          isOpen={universalFormOpen}
          onClose={handleUniversalFormClose}
          onSave={handleUniversalProviderSave}
          initialPreset={selectedUniversalPreset}
        />
      )}

      {unsavedGuard.confirmDialog}
    </FullScreenPanel>
  );
}
