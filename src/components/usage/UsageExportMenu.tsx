import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { usageApi } from "@/lib/api/usage";
import { extractErrorMessage } from "@/utils/errorUtils";
import type {
  LogFilters,
  UsageExportFormat,
  UsageExportKind,
} from "@/types/usage";

/** 与后端 MAX_EXPORT_ROWS 保持一致，仅用于提示文案 */
const MAX_EXPORT_ROWS = 200_000;

interface ExportEntry {
  kind: UsageExportKind;
  format: UsageExportFormat;
  labelKey: string;
  defaultLabel: string;
}

const EXPORT_ENTRIES: ExportEntry[] = [
  {
    kind: "logs",
    format: "csv",
    labelKey: "usage.export.logsCsv",
    defaultLabel: "请求日志 (CSV)",
  },
  {
    kind: "logs",
    format: "json",
    labelKey: "usage.export.logsJson",
    defaultLabel: "请求日志 (JSON)",
  },
  {
    kind: "statement",
    format: "csv",
    labelKey: "usage.export.statementCsv",
    defaultLabel: "账单汇总 (CSV)",
  },
  {
    kind: "statement",
    format: "json",
    labelKey: "usage.export.statementJson",
    defaultLabel: "账单汇总 (JSON)",
  },
];

export function buildUsageExportFileName(
  kind: UsageExportKind,
  format: UsageExportFormat,
  now: Date = new Date(),
): string {
  const stamp = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(
    2,
    "0",
  )}${String(now.getDate()).padStart(2, "0")}`;
  return `cc-switch-usage-${kind}-${stamp}.${format}`;
}

interface UsageExportMenuProps {
  /** 当前仪表盘筛选条件（时间范围、应用、来源、模型），导出时原样传给后端 */
  filters: LogFilters;
}

export function UsageExportMenu({ filters }: UsageExportMenuProps) {
  const { t } = useTranslation();
  const [isExporting, setIsExporting] = useState(false);

  const handleExport = async (
    kind: UsageExportKind,
    format: UsageExportFormat,
  ) => {
    if (isExporting) return;
    setIsExporting(true);
    try {
      const targetPath = await usageApi.saveUsageExportDialog(
        buildUsageExportFileName(kind, format),
        format,
      );
      if (!targetPath) return;
      const result = await usageApi.exportUsageData({
        kind,
        format,
        filters,
        targetPath,
      });
      toast.success(
        t("usage.export.success", {
          defaultValue: "已导出 {{rows}} 行到 {{path}}",
          rows: result.rows,
          path: result.path,
        }),
      );
      if (result.truncated) {
        toast.warning(
          t("usage.export.truncated", {
            defaultValue:
              "日志超过 {{max}} 行，仅导出了前 {{max}} 行，请缩小时间范围",
            max: MAX_EXPORT_ROWS,
          }),
        );
      }
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("usage.export.failed", { defaultValue: "导出失败" }),
      );
    } finally {
      setIsExporting(false);
    }
  };

  const menuLabel = t("usage.export.menu", { defaultValue: "导出" });

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="outline"
          className="h-9 gap-1.5 text-xs"
          disabled={isExporting}
          title={t("usage.export.tooltip", {
            defaultValue: "按当前筛选条件导出请求日志或账单汇总",
          })}
          aria-label={menuLabel}
        >
          {isExporting ? (
            <Loader2 className="h-4 w-4 shrink-0 animate-spin" />
          ) : (
            <Download className="h-4 w-4 shrink-0" />
          )}
          <span className="hidden sm:inline">{menuLabel}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
          {t("usage.export.scopeHint", {
            defaultValue: "使用当前时间范围与筛选条件",
          })}
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        {EXPORT_ENTRIES.map((entry) => (
          <DropdownMenuItem
            key={`${entry.kind}-${entry.format}`}
            onSelect={() => void handleExport(entry.kind, entry.format)}
          >
            {t(entry.labelKey, { defaultValue: entry.defaultLabel })}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
