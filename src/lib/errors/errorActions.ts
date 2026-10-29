import type { TFunction } from "i18next";
import { toast } from "sonner";
import { backupsApi, settingsApi } from "@/lib/api/settings";
import { failoverApi } from "@/lib/api/failover";
import { proxyApi } from "@/lib/api/proxy";
import { IpcError, toIpcError } from "@/lib/api/ipc";
import { queryClient } from "@/lib/query/queryClient";
import { extractErrorMessage } from "@/utils/errorUtils";

/** 与 sonner `toast(..., { action })` 的形态一致 */
export interface IpcErrorAction {
  label: string;
  onClick: () => void;
}

const readString = (details: unknown, key: string): string | null => {
  if (!details || typeof details !== "object") return null;
  const value = (details as Record<string, unknown>)[key];
  return typeof value === "string" && value.trim() ? value : null;
};

/** 熔断：指定供应商则只重置它，否则重置该应用故障转移队列中的全部供应商 */
const resetCircuit = async (appType: string, providerId: string | null) => {
  if (providerId) {
    await failoverApi.resetCircuitBreaker(providerId, appType);
    return;
  }
  const queue = await failoverApi.getFailoverQueue(appType);
  for (const item of queue) {
    await failoverApi.resetCircuitBreaker(item.providerId, appType);
  }
};

/** 恢复指定备份；未指定时恢复最新的一份（恢复前后端会先做安全备份） */
const restoreBackup = async (backupId: string | null, t: TFunction) => {
  let filename = backupId
    ? backupId.endsWith(".db")
      ? backupId
      : `${backupId}.db`
    : null;
  if (!filename) {
    // 后端按创建时间倒序返回
    const [latest] = await backupsApi.listDbBackups();
    if (!latest) {
      throw new Error(t("errors.actions.noBackup"));
    }
    filename = latest.filename;
  }
  await backupsApi.restoreDbBackup(filename);
};

const withFeedback =
  (label: string, run: () => Promise<void>, t: TFunction) => (): void => {
    void run().then(
      () => {
        // 修复操作绕过了各自的 mutation hook，这里统一失效缓存，让界面反映新状态
        void queryClient.invalidateQueries();
        toast.success(t("errors.actions.succeeded", { action: label }));
      },
      (error: unknown) => {
        toast.error(
          t("errors.actions.failed", {
            action: label,
            error: extractErrorMessage(error) || t("common.unknown"),
          }),
        );
      },
    );
  };

/**
 * 按 `IpcError.code` 为错误 toast 提供修复按钮，所需参数取自 `details`。
 * 未映射的 code、缺少必要参数或非结构化错误返回 null，调用方保持原样。
 */
export const getIpcErrorAction = (
  error: unknown,
  t: TFunction,
): IpcErrorAction | null => {
  const normalized = toIpcError(error);
  if (!(normalized instanceof IpcError)) return null;
  const { code, details } = normalized;

  const make = (label: string, run: () => Promise<void>): IpcErrorAction => ({
    label,
    onClick: withFeedback(label, run, t),
  });

  switch (code) {
    case "TOOL_NOT_INSTALLED": {
      const tool = readString(details, "tool");
      if (!tool) return null;
      return make(t("errors.actions.install"), () =>
        settingsApi.runToolLifecycleAction([tool], "install"),
      );
    }
    case "CIRCUIT_OPEN": {
      const appType = readString(details, "appType");
      if (!appType) return null;
      const providerId = readString(details, "providerId");
      return make(t("errors.actions.resetCircuit"), () =>
        resetCircuit(appType, providerId),
      );
    }
    case "TAKEOVER_ACTIVE": {
      const appType = readString(details, "appType");
      if (!appType) return null;
      return make(t("errors.actions.stopTakeover"), () =>
        proxyApi.setProxyTakeoverForApp(appType, false),
      );
    }
    case "DB_VERSION_TOO_NEW": {
      const backupId = readString(details, "backupId");
      return make(t("errors.actions.restoreBackup"), () =>
        restoreBackup(backupId, t),
      );
    }
    default:
      return null;
  }
};
