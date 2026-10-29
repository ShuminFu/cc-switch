import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import type { Provider } from "@/types";
import { providersApi, type AppId } from "@/lib/api";
import { hermesApi } from "@/lib/api/hermes";
import { invalidateProviderGraph } from "@/entities/provider/invalidate";
import { extractErrorMessage } from "@/utils/errorUtils";
import { ConfirmDialog } from "@/components/ConfirmDialog";

/** "remove"：仅从 live 配置移除（累加模式应用）；"delete"：从数据库删除 */
export type ProviderConfirmKind = "remove" | "delete";

export interface ProviderConfirmRequest {
  provider: Provider;
  action: ProviderConfirmKind;
}

/**
 * 供应商删除 / 移出配置的确认流程：`request` 打开确认框，`confirm` 执行对应操作。
 */
export function useProviderConfirm(
  activeApp: AppId,
  deleteProvider: (id: string) => Promise<unknown>,
) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [pending, setPending] = useState<ProviderConfirmRequest | null>(null);

  const confirm = async () => {
    if (!pending) return;
    const { provider, action } = pending;

    if (action === "remove") {
      // Remove from live config only (for additive mode apps like OpenCode/OpenClaw)
      // Does NOT delete from database - provider remains in the list
      await providersApi.removeFromLiveConfig(provider.id, activeApp);
      // Invalidate the provider graph (incl. live ids) to refresh the isInConfig state
      await invalidateProviderGraph(queryClient, activeApp);
      toast.success(
        t("notifications.removeFromConfigSuccess", {
          defaultValue: "已从配置移除",
        }),
        { closeButton: true },
      );
    } else {
      await deleteProvider(provider.id);
    }
    setPending(null);
  };

  return {
    pending,
    request: (provider: Provider, action: ProviderConfirmKind) =>
      setPending({ provider, action }),
    confirm,
    cancel: () => setPending(null),
  };
}

export function ProviderConfirmDialog({
  pending,
  onConfirm,
  onCancel,
}: {
  pending: ProviderConfirmRequest | null;
  onConfirm: () => Promise<void>;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const isRemove = pending?.action === "remove";
  const name = pending?.provider.name;

  return (
    <ConfirmDialog
      isOpen={Boolean(pending)}
      title={
        isRemove ? t("confirm.removeProvider") : t("confirm.deleteProvider")
      }
      message={
        pending
          ? isRemove
            ? t("confirm.removeProviderMessage", { name })
            : t("confirm.deleteProviderMessage", { name })
          : ""
      }
      onConfirm={() => void onConfirm()}
      onCancel={onCancel}
    />
  );
}

/** Hermes Web UI 未运行时，确认后启动 Hermes Dashboard */
export function HermesLaunchConfirmDialog({
  isOpen,
  onClose,
}: {
  isOpen: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();

  const launch = async () => {
    try {
      await hermesApi.launchDashboard();
      toast.success(t("hermes.webui.launching"));
    } catch (error) {
      toast.error(t("hermes.webui.launchFailed"), {
        description: extractErrorMessage(error) || undefined,
      });
    }
  };

  return (
    <ConfirmDialog
      isOpen={isOpen}
      title={t("hermes.webui.launchConfirmTitle")}
      message={t("hermes.webui.launchConfirmMessage")}
      confirmText={t("hermes.webui.launchConfirmAction")}
      variant="info"
      onConfirm={() => {
        onClose();
        void launch();
      }}
      onCancel={onClose}
    />
  );
}
