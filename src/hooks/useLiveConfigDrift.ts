import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { TFunction } from "i18next";
import { driftApi, LIVE_CONFIG_DRIFT_EVENT, type AppId } from "@/lib/api";
import { appLabel } from "@/entities/app/registry";
import { extractErrorMessage } from "@/utils/errorUtils";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import type { DriftReport, DriftResolution } from "@/shared/bindings";

/** 提示里最多列出的字段数 */
const MAX_LISTED_FIELDS = 3;

export interface DriftPrompt {
  message: string;
  /** 主操作：修复占位符或并入通用配置；没有可执行的主操作时为 `undefined` */
  primary?: { label: string; resolution: DriftResolution };
}

/** 把漂移报告转换为提示文案与可选的主操作 */
export function buildDriftPrompt(
  report: DriftReport,
  t: TFunction,
): DriftPrompt {
  const app = appLabel(report.app);
  if (report.leftoverPlaceholder) {
    return {
      message: t("notifications.liveDriftPlaceholder", { app }),
      primary: {
        label: t("notifications.liveDriftRepair"),
        resolution: { kind: "reconcile" },
      },
    };
  }

  const names = report.fields.map((field) => field.path.join(".") || "*");
  const listed = names.slice(0, MAX_LISTED_FIELDS).join(", ");
  const fields =
    names.length > MAX_LISTED_FIELDS
      ? `${listed}, +${names.length - MAX_LISTED_FIELDS}`
      : listed;
  const foldable = report.fields
    .filter((field) => field.foldable)
    .map((field) => field.path);

  return {
    message: t("notifications.liveDriftExternal", { app, fields }),
    primary:
      foldable.length > 0
        ? {
            label: t("notifications.liveDriftFold"),
            resolution: { kind: "fold", paths: foldable },
          }
        : undefined,
  };
}

/** 订阅后端的 live 配置漂移事件，以带操作按钮的提示让用户修复 / 并入 / 忽略 */
export function useLiveConfigDrift(): void {
  const { t } = useTranslation();

  useTauriEvent<DriftReport>(LIVE_CONFIG_DRIFT_EVENT, (report) => {
    if (report.ignored) return;
    const app = appLabel(report.app);
    const resolve = async (resolution: DriftResolution) => {
      try {
        await driftApi.resolve(report.app as AppId, resolution);
        if (resolution.kind !== "ignore") {
          toast.success(t("notifications.liveDriftResolved", { app }));
        }
      } catch (error) {
        toast.error(
          t("notifications.liveDriftResolveFailed", {
            app,
            error: extractErrorMessage(error),
          }),
        );
      }
    };

    const prompt = buildDriftPrompt(report, t);
    toast.warning(prompt.message, {
      id: `live-config-drift-${report.app}`,
      duration: Infinity,
      action: prompt.primary
        ? {
            label: prompt.primary.label,
            onClick: () => void resolve(prompt.primary!.resolution),
          }
        : undefined,
      cancel: {
        label: t("notifications.liveDriftIgnore"),
        onClick: () => void resolve({ kind: "ignore" }),
      },
    });
  });
}
