import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import {
  providersApi,
  settingsApi,
  type AppId,
  type ProviderSwitchEvent,
} from "@/lib/api";
import { providerKeys } from "@/entities/provider/keys";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { useLiveConfigDrift } from "@/hooks/useLiveConfigDrift";

interface SyncStatusUpdatedPayload {
  source?: string;
  status?: string;
  error?: string;
}

/**
 * 主窗口级的后端事件订阅与启动时一次性提示：
 * 供应商切换 / 统一供应商同步 / 项目应用 / 云同步状态 / 代理官方供应商警告 / live 配置漂移，
 * 以及配置迁移、Skills 迁移结果与启动诊断报告中的失败步骤。
 */
export function useAppShellEvents(
  activeApp: AppId,
  refetchProviders: () => Promise<unknown>,
): void {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let active = true;

    const setupListener = async () => {
      try {
        const off = await providersApi.onSwitched(
          async (event: ProviderSwitchEvent) => {
            if (event.appType === activeApp) {
              await refetchProviders();
            } else {
              // 供应商列表带 staleTime，非当前应用（如托盘切换）需显式标记过期
              await queryClient.invalidateQueries({
                queryKey: providerKeys.list(event.appType),
              });
            }
          },
        );
        if (!active) {
          off();
          return;
        }
        unsubscribe = off;
      } catch (error) {
        console.error("[App] Failed to subscribe provider switch event", error);
      }
    };

    void setupListener();
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [activeApp, refetchProviders, queryClient]);

  useTauriEvent("universal-provider-synced", async () => {
    await queryClient.invalidateQueries({ queryKey: providerKeys.all });
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.error("[App] Failed to update tray menu", error);
    }
  });

  // 应用项目后刷新相关缓存（providers 由既有 provider-switched 监听承接；
  // proxy 状态由后端直接改 DB，不走 mutation，必须显式刷新）
  useTauriEvent("profile-applied", async () => {
    await queryClient.invalidateQueries({ queryKey: ["profiles"] });
    await queryClient.invalidateQueries({ queryKey: ["mcp", "all"] });
    await queryClient.invalidateQueries({ queryKey: ["skills"] });
    await queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] });
    await queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    await queryClient.invalidateQueries({
      queryKey: providerKeys.list("claude-desktop"),
    });
  });

  useTauriEvent<SyncStatusUpdatedPayload | null | undefined>(
    "webdav-sync-status-updated",
    async (payload) => {
      const statusPayload = payload ?? {};
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      if (statusPayload.source !== "auto" || statusPayload.status !== "error") {
        return;
      }
      toast.error(
        t("settings.webdavSync.autoSyncFailedToast", {
          error: statusPayload.error || t("common.unknown"),
        }),
      );
    },
  );

  useTauriEvent<SyncStatusUpdatedPayload | null | undefined>(
    "s3-sync-status-updated",
    async (payload) => {
      const statusPayload = payload ?? {};
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      if (statusPayload.source !== "auto" || statusPayload.status !== "error") {
        return;
      }
      toast.error(
        t("settings.s3Sync.autoSyncFailedToast", {
          error: statusPayload.error || t("common.unknown"),
        }),
      );
    },
  );

  // live 配置漂移（残留占位符 / 外部改动）提示
  useLiveConfigDrift();

  useTauriEvent<{ appType: string; providerName: string }>(
    "proxy-official-warning",
    (payload) => {
      toast.warning(
        t("notifications.proxyOfficialWarning", {
          name: payload.providerName,
        }),
        { duration: 8000 },
      );
    },
  );

  useEffect(() => {
    const checkMigration = async () => {
      try {
        const migrated = await settingsApi.getMigrationResult();
        if (migrated) {
          toast.success(t("migration.success"), { closeButton: true });
        }
      } catch (error) {
        console.error("[App] Failed to check migration result:", error);
      }
    };

    checkMigration();
  }, [t]);

  useEffect(() => {
    const checkSkillsMigration = async () => {
      try {
        const result = await settingsApi.getSkillsMigrationResult();
        if (result?.error) {
          toast.error(t("migration.skillsFailed"), {
            description: t("migration.skillsFailedDescription"),
            closeButton: true,
          });
          console.error("[App] Skills SSOT migration failed:", result.error);
          return;
        }
        if (result && result.count > 0) {
          toast.success(t("migration.skillsSuccess", { count: result.count }), {
            closeButton: true,
          });
          await queryClient.invalidateQueries({ queryKey: ["skills"] });
        }
      } catch (error) {
        console.error("[App] Failed to check skills migration result:", error);
      }
    };

    checkSkillsMigration();
  }, [t, queryClient]);

  // 启动诊断：迁移 / live 导入等步骤失败时提示，避免表现为无声的"供应商丢失"
  useEffect(() => {
    const checkStartupReport = async () => {
      try {
        const report = await settingsApi.getStartupReport();
        const failed = report.steps.filter((step) => step.status === "failed");
        if (failed.length > 0) {
          toast.warning(
            t("settings.advanced.startupReport.toastTitle", {
              count: failed.length,
            }),
            {
              description: t(
                "settings.advanced.startupReport.toastDescription",
              ),
              closeButton: true,
              duration: 10000,
            },
          );
          console.warn("[App] Startup steps failed:", failed);
        }
      } catch (error) {
        console.error("[App] Failed to load startup report:", error);
      }
    };

    checkStartupReport();
  }, [t]);
}
