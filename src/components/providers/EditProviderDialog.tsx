import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Provider } from "@/types";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { openclawApi, providersApi, vscodeApi, type AppId } from "@/lib/api";
import { useUnsavedChangesGuard } from "@/features/providers/forms/useUnsavedChangesGuard";

interface EditProviderDialogProps {
  open: boolean;
  provider: Provider | null;
  onOpenChange: (open: boolean) => void;
  onSubmit: (payload: {
    provider: Provider;
    originalId?: string;
  }) => Promise<void> | void;
  appId: AppId;
  isProxyTakeover?: boolean; // 代理接管模式下不读取 live（避免显示被接管后的代理配置）
}

export function EditProviderDialog({
  open,
  provider,
  onOpenChange,
  onSubmit,
  appId,
  isProxyTakeover = false,
}: EditProviderDialogProps) {
  const { t } = useTranslation();
  const [isFormSubmitting, setIsFormSubmitting] = useState(false);
  const closePanel = useCallback(() => onOpenChange(false), [onOpenChange]);
  // 用户主动关闭（Esc/返回）时若有未保存修改先确认；保存成功后直接关闭
  const unsavedGuard = useUnsavedChangesGuard(closePanel);

  // 默认使用传入的 provider.settingsConfig，若当前编辑对象是"当前生效供应商"，则尝试读取实时配置替换初始值
  const [liveSettings, setLiveSettings] = useState<Record<
    string,
    unknown
  > | null>(null);

  // 记录已完成实时配置读取的 key（appId + provider.id），防止重复读取覆盖用户编辑。
  // 按 key 记录而非布尔值：被取消的读取（如切换了 provider）不会遗留"已加载"状态，
  // 新一轮 effect 会为新的 key 重新读取，不会卡在加载占位
  const [loadedLiveKey, setLoadedLiveKey] = useState<string | null>(null);

  const liveKey = open && provider ? `${appId}:${provider.id}` : null;

  // 代理接管模式 / OpenCode 不读取 live，无需等待，表单可立即渲染
  const skipLiveRead = isProxyTakeover || appId === "opencode";

  // 表单只在实时配置读取完成后才渲染：ProviderForm 及 Codex/Gemini 配置 hooks
  // 只在挂载时消费 initialData，若先用数据库配置渲染、待 live 返回后再 reset，
  // 会抹掉用户在此期间输入的内容
  const hasLoadedLive =
    liveKey !== null && (loadedLiveKey === liveKey || skipLiveRead);

  useEffect(() => {
    let cancelled = false;
    const finish = (settings: Record<string, unknown> | null) => {
      if (cancelled || !liveKey) return;
      setLiveSettings(settings);
      setLoadedLiveKey(liveKey);
    };
    const load = async () => {
      if (!open || !provider || !liveKey) {
        setLiveSettings(null);
        setLoadedLiveKey(null);
        return;
      }

      // 关键修复：只在首次打开时加载一次
      if (loadedLiveKey === liveKey) {
        return;
      }

      // 代理接管模式：Live 配置已被代理改写，读取 live 会导致编辑界面展示代理地址/占位符等内容
      // 因此直接回退到 SSOT（数据库）配置，避免用户困惑与误保存
      // OpenCode uses additive mode - each provider's config is stored independently in DB
      // Reading live config would return the full opencode.json (with $schema, provider, mcp etc.)
      // instead of just the provider fragment, causing incorrect nested structure on save
      if (skipLiveRead) {
        finish(null);
        return;
      }

      if (appId === "openclaw") {
        try {
          const live = await openclawApi.getLiveProvider(provider.id);
          finish(live && typeof live === "object" ? live : null);
        } catch {
          finish(null);
        }
        return;
      }

      try {
        const currentId = await providersApi.getCurrent(appId);
        if (currentId && provider.id === currentId) {
          const live = (await vscodeApi.getLiveProviderSettings(
            appId,
          )) as Record<string, unknown>;
          finish(live && typeof live === "object" ? live : null);
        } else {
          finish(null);
        }
      } catch {
        // 读取实时配置失败则回退到 SSOT（不打断编辑流程）
        finish(null);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [open, provider?.id, appId, liveKey, loadedLiveKey, skipLiveRead]); // 只依赖 provider.id，不依赖整个 provider 对象

  // 仅使用属于当前 key 的实时配置，避免切换 provider 时短暂套用上一个的 live 值
  const effectiveLiveSettings =
    loadedLiveKey !== null && loadedLiveKey === liveKey ? liveSettings : null;

  const initialSettingsConfig = useMemo(() => {
    const base = (effectiveLiveSettings ??
      provider?.settingsConfig ??
      {}) as Record<string, unknown>;

    // Codex 的 modelCatalog 是 cc-switch 私有字段，SSOT 在数据库。Live 的 config.toml
    // 仅在写入时投影出 model_catalog_json 指针；Codex.app 改写配置、代理接管/恢复周期、
    // 来回切换供应商都可能让 Live 丢失该投影，从而 read_live_settings 反解为空。
    // 若放任 Live 覆盖，编辑界面会显示空映射表，保存后连同数据库里的映射一起清空（数据丢失）。
    // 因此始终以数据库 SSOT 的 modelCatalog 为准，仅在数据库确实没有时才回退到 Live 反解结果。
    if (
      appId === "codex" &&
      effectiveLiveSettings &&
      provider?.settingsConfig &&
      typeof provider.settingsConfig === "object"
    ) {
      const dbCatalog = (provider.settingsConfig as Record<string, unknown>)
        .modelCatalog;
      if (dbCatalog !== undefined) {
        return { ...base, modelCatalog: dbCatalog };
      }
    }

    return base;
  }, [effectiveLiveSettings, provider?.settingsConfig, appId]); // 只依赖 settingsConfig，不依赖整个 provider

  // 固定 initialData，防止 provider 对象更新时重置表单
  const initialData = useMemo(() => {
    if (!provider) return null;
    return {
      name: provider.name,
      notes: provider.notes,
      websiteUrl: provider.websiteUrl,
      settingsConfig: initialSettingsConfig,
      category: provider.category,
      meta: provider.meta,
      icon: provider.icon,
      iconColor: provider.iconColor,
    };
  }, [
    open, // 修复：编辑保存后再次打开显示旧数据，依赖 open 确保每次打开时重新读取最新 provider 数据
    provider?.id, // 只依赖 ID，provider 对象更新不会触发重新计算
    provider?.meta, // 供应商元数据变化时重新初始化表单
    initialSettingsConfig,
  ]);

  const handleSubmit = useCallback(
    async (values: ProviderFormValues) => {
      if (!provider) return;

      // 注意：values.settingsConfig 已经是最终的配置字符串
      // ProviderForm 已经为不同的 app 类型（Claude/Codex/Gemini）正确组装了配置
      const parsedConfig = JSON.parse(values.settingsConfig) as Record<
        string,
        unknown
      >;
      const nextProviderId =
        (appId === "opencode" || appId === "openclaw") &&
        values.providerKey?.trim()
          ? values.providerKey.trim()
          : provider.id;

      const updatedProvider: Provider = {
        ...provider,
        id: nextProviderId,
        name: values.name.trim(),
        notes: values.notes?.trim() || undefined,
        websiteUrl: values.websiteUrl?.trim() || undefined,
        settingsConfig: parsedConfig,
        icon: values.icon?.trim() || undefined,
        iconColor: values.iconColor?.trim() || undefined,
        ...(values.presetCategory ? { category: values.presetCategory } : {}),
        // 保留或更新 meta 字段
        ...(values.meta ? { meta: values.meta } : {}),
      };

      await onSubmit({
        provider: updatedProvider,
        originalId: provider.id,
      });
      onOpenChange(false);
    },
    [appId, onSubmit, onOpenChange, provider],
  );

  if (!provider || !initialData) {
    return null;
  }

  return (
    <FullScreenPanel
      isOpen={open}
      title={t("provider.editProvider")}
      onClose={unsavedGuard.requestClose}
      footer={
        <Button
          type="submit"
          form="provider-form"
          disabled={isFormSubmitting || !hasLoadedLive}
          className="bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Save className="h-4 w-4 mr-2" />
          {t("common.save")}
        </Button>
      }
    >
      {hasLoadedLive ? (
        <ProviderForm
          appId={appId}
          providerId={provider.id}
          submitLabel={t("common.save")}
          onSubmit={handleSubmit}
          onCancel={unsavedGuard.requestClose}
          onSubmittingChange={setIsFormSubmitting}
          onDirtyChange={unsavedGuard.onDirtyChange}
          initialData={initialData}
          showButtons={false}
          isProxyTakeover={isProxyTakeover}
        />
      ) : (
        <div
          className="flex items-center justify-center gap-2 py-16 text-sm text-muted-foreground"
          role="status"
          data-testid="edit-provider-loading"
        >
          <Loader2 className="h-4 w-4 animate-spin" />
          {t("common.loading")}
        </div>
      )}

      {unsavedGuard.confirmDialog}
    </FullScreenPanel>
  );
}
