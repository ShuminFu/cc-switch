import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";
import type { UsageParity } from "@/shared/bindings";

interface UsageParityBadgeProps {
  parity: UsageParity;
}

/**
 * 规范化 usage 与上游原始 usage 块不一致时，在请求日志行上显示的徽标。
 * 悬停/读屏给出不一致类型与两边的 token 总量。
 */
export function UsageParityBadge({ parity }: UsageParityBadgeProps) {
  const { t } = useTranslation();
  const issue = t(`usage.parity.issue.${parity.issue}`);
  const detail =
    parity.rawTotal == null
      ? t("usage.parity.detailNoRaw", {
          normalized: parity.normalizedTotal,
        })
      : t("usage.parity.detail", {
          normalized: parity.normalizedTotal,
          raw: parity.rawTotal,
          input: parity.rawInput ?? 0,
          cache: parity.rawCache ?? 0,
        });
  const description = `${issue}: ${detail}`;

  return (
    <span
      role="status"
      aria-label={`${t("usage.parity.badge")} - ${description}`}
      title={description}
      className="ml-1 inline-flex items-center gap-0.5 rounded-full border border-amber-500/40 bg-amber-500/10 px-1.5 py-0 align-middle text-[10px] font-medium text-amber-700 dark:text-amber-400"
    >
      <AlertTriangle className="h-3 w-3" aria-hidden="true" />
      {t("usage.parity.badge")}
    </span>
  );
}
