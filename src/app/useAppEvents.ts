import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { providersApi } from "@/lib/api";
import { queryKeys } from "@/lib/query/keys";
import { useTauriEvent } from "@/hooks/useTauriEvent";

import type { SyncStatusUpdatedPayload } from "@/lib/events";

/**
 * App-wide backend event subscriptions and one-time startup checks.
 * Mounted once by the shell, independent of the current page.
 */
export function useAppEvents() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  // Tray / failover / deep-link switches: refresh that app's provider list
  // whether or not the providers page is mounted.
  useTauriEvent("provider-switched", async (event) => {
    await queryClient.invalidateQueries({
      queryKey: queryKeys.providers.byApp(event.appType),
    });
  });

  useTauriEvent("universal-provider-synced", async () => {
    await queryClient.invalidateQueries({ queryKey: queryKeys.providers.all });
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.error("[App] Failed to update tray menu", error);
    }
  });

  // 应用项目后刷新相关缓存（providers 由既有 provider-switched 监听承接；
  // proxy 状态由后端直接改 DB，不走 mutation，必须显式刷新）
  useTauriEvent("profile-applied", async () => {
    await Promise.all(
      [
        queryKeys.profiles.all,
        queryKeys.mcp.all,
        queryKeys.skills.all,
        queryKeys.proxy.takeoverStatus,
        queryKeys.proxy.status,
        queryKeys.providers.byApp("claude-desktop"),
      ].map((queryKey) => queryClient.invalidateQueries({ queryKey })),
    );
  });

  const handleSyncStatus =
    (failedToastKey: string) =>
    async (payload: SyncStatusUpdatedPayload | null | undefined) => {
      const statusPayload = payload ?? {};
      await queryClient.invalidateQueries({ queryKey: queryKeys.settings.all });
      if (statusPayload.source !== "auto" || statusPayload.status !== "error") {
        return;
      }
      toast.error(
        t(failedToastKey, {
          error: statusPayload.error || t("common.unknown"),
        }),
      );
    };

  useTauriEvent(
    "webdav-sync-status-updated",
    handleSyncStatus("settings.webdavSync.autoSyncFailedToast"),
  );

  useTauriEvent(
    "s3-sync-status-updated",
    handleSyncStatus("settings.s3Sync.autoSyncFailedToast"),
  );

  useTauriEvent("proxy-official-warning", (payload) => {
    toast.warning(
      t("notifications.proxyOfficialWarning", {
        name: payload.providerName,
        defaultValue: `当前供应商 ${payload.providerName} 是官方供应商，建议切换到第三方供应商后再使用代理接管`,
      }),
      { duration: 8000 },
    );
  });

  useEffect(() => {
    const checkMigration = async () => {
      try {
        const migrated = await invoke<boolean>("get_migration_result");
        if (migrated) {
          toast.success(
            t("migration.success", { defaultValue: "配置迁移成功" }),
            { closeButton: true },
          );
        }
      } catch (error) {
        console.error("[App] Failed to check migration result:", error);
      }
    };

    void checkMigration();
  }, [t]);

  useEffect(() => {
    const checkSkillsMigration = async () => {
      try {
        const result = await invoke<{ count: number; error?: string } | null>(
          "get_skills_migration_result",
        );
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
          await queryClient.invalidateQueries({
            queryKey: queryKeys.skills.all,
          });
        }
      } catch (error) {
        console.error("[App] Failed to check skills migration result:", error);
      }
    };

    void checkSkillsMigration();
  }, [t, queryClient]);
}
