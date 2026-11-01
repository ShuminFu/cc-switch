import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Download, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { usageApi } from "@/lib/api/usage";
import { resolveUsageRange } from "@/lib/usageRange";
import type { LogFilters, UsageRangeSelection } from "@/types/usage";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  defaultExportFileName,
  fetchAllRequestLogs,
  logsToCsv,
  logsToJson,
  type UsageExportFormat,
} from "./usageExport";

interface UsageExportButtonProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
}

/** Dashboard-header export of the request logs matching the active filters. */
export function UsageExportButton({
  range,
  appType,
  providerName,
  model,
}: UsageExportButtonProps) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);

  const runExport = async (format: UsageExportFormat) => {
    if (busy) return;
    setBusy(true);
    try {
      const filters: LogFilters = {
        appType: appType && appType !== "all" ? appType : undefined,
        providerName,
        model,
        ...resolveUsageRange(range),
      };
      const { logs } = await fetchAllRequestLogs(filters);
      const content = format === "csv" ? logsToCsv(logs) : logsToJson(logs);
      const saved = await usageApi.saveUsageExport(
        defaultExportFileName(format),
        format,
        content,
      );
      // A cancelled save dialog is not an error.
      if (!saved) return;
      toast.success(
        t("usage.exportSuccess", {
          defaultValue: "Exported {{count}} requests to {{path}}",
          count: logs.length,
          path: saved,
        }),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(
        t("usage.exportFailed", {
          defaultValue: "Export failed: {{message}}",
          message: extractErrorMessage(error) || String(error),
        }),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          className="h-9 gap-1.5 text-xs"
          disabled={busy}
          aria-label={t("usage.export", { defaultValue: "Export" })}
        >
          {busy ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Download className="h-3.5 w-3.5" />
          )}
          {t("usage.export", { defaultValue: "Export" })}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onSelect={() => void runExport("csv")}>
          {t("usage.exportCsv", { defaultValue: "Request logs as CSV" })}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => void runExport("json")}>
          {t("usage.exportJson", { defaultValue: "Request logs as JSON" })}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
