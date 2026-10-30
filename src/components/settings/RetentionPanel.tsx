import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, Wrench } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { formatBytes } from "@/components/settings/BackupListSection";
import { maintenanceApi } from "@/lib/api";
import { usageKeys } from "@/lib/query/usage";
import { extractErrorMessage } from "@/utils/errorUtils";

/** 与后端 `RetentionPolicy::DEFAULT_DETAIL_DAYS` 一致 */
export const DEFAULT_DETAIL_RETENTION_DAYS = 30;
/** `usageRollupRetentionDays` 为 0 / 未设置表示永久保留 */
export const KEEP_ROLLUPS_FOREVER = 0;

const DETAIL_DAY_OPTIONS = [7, 14, 30, 60, 90, 180, 365];
const ROLLUP_DAY_OPTIONS = [90, 180, 365, 730, 1095];

export const dbStorageStatsQueryKey = ["db-storage-stats"] as const;

/** 选项中补上当前值（例如手动写进 settings.json 的天数），避免下拉框显示空白 */
function withCurrent(options: number[], current: number): number[] {
  return options.includes(current)
    ? options
    : [...options, current].sort((a, b) => a - b);
}

interface RetentionPanelProps {
  usageDetailRetentionDays?: number;
  usageRollupRetentionDays?: number;
  onSettingsChange: (updates: {
    usageDetailRetentionDays?: number;
    usageRollupRetentionDays?: number;
  }) => void | Promise<unknown>;
}

export function RetentionPanel({
  usageDetailRetentionDays,
  usageRollupRetentionDays,
  onSettingsChange,
}: RetentionPanelProps) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();

  const statsQuery = useQuery({
    queryKey: dbStorageStatsQueryKey,
    queryFn: () => maintenanceApi.getDbStorageStats(),
  });

  const maintenance = useMutation({
    mutationFn: () => maintenanceApi.runDbMaintenance(),
    onSuccess: async (report) => {
      toast.success(t("settings.advanced.retention.maintenanceDone"), {
        description: t("settings.advanced.retention.maintenanceSummary", {
          detail: report.detailRowsRolledUp,
          rollups: report.rollupRowsDeleted,
          checks: report.streamCheckLogsDeleted,
          backups: report.backupsDeleted,
        }),
      });
      await queryClient.invalidateQueries({ queryKey: dbStorageStatsQueryKey });
      await queryClient.invalidateQueries({ queryKey: ["db-backups"] });
      await queryClient.invalidateQueries({ queryKey: usageKeys.all });
    },
    onError: (error) => {
      toast.error(
        extractErrorMessage(error) ||
          t("settings.advanced.retention.maintenanceFailed"),
      );
    },
  });

  const detailDays = Math.max(
    1,
    usageDetailRetentionDays ?? DEFAULT_DETAIL_RETENTION_DAYS,
  );
  const rollupDays = usageRollupRetentionDays ?? KEEP_ROLLUPS_FOREVER;
  const numberFormat = new Intl.NumberFormat(i18n.language);
  const stats = statsQuery.data;

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-2 gap-4">
        <div className="space-y-2">
          <Label htmlFor="retention-detail-days" className="text-sm">
            {t("settings.advanced.retention.detailDaysLabel")}
          </Label>
          <Select
            value={String(detailDays)}
            onValueChange={(v) =>
              onSettingsChange({ usageDetailRetentionDays: Number(v) })
            }
          >
            <SelectTrigger id="retention-detail-days" className="h-9">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {withCurrent(DETAIL_DAY_OPTIONS, detailDays).map((days) => (
                <SelectItem key={days} value={String(days)}>
                  {t("settings.advanced.retention.days", { days })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.retention.detailDaysHint")}
          </p>
        </div>

        <div className="space-y-2">
          <Label htmlFor="retention-rollup-days" className="text-sm">
            {t("settings.advanced.retention.rollupDaysLabel")}
          </Label>
          <Select
            value={String(rollupDays)}
            onValueChange={(v) =>
              onSettingsChange({ usageRollupRetentionDays: Number(v) })
            }
          >
            <SelectTrigger id="retention-rollup-days" className="h-9">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={String(KEEP_ROLLUPS_FOREVER)}>
                {t("settings.advanced.retention.keepForever")}
              </SelectItem>
              {(rollupDays === KEEP_ROLLUPS_FOREVER
                ? ROLLUP_DAY_OPTIONS
                : withCurrent(ROLLUP_DAY_OPTIONS, rollupDays)
              ).map((days) => (
                <SelectItem key={days} value={String(days)}>
                  {t("settings.advanced.retention.days", { days })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.retention.rollupDaysHint")}
          </p>
        </div>
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <h4 className="text-sm font-medium">
            {t("settings.advanced.retention.storageTitle")}
          </h4>
          <Button
            variant="outline"
            size="sm"
            className="h-7 px-2 text-xs"
            disabled={maintenance.isPending}
            onClick={() => maintenance.mutate()}
          >
            {maintenance.isPending ? (
              <Loader2 className="h-3 w-3 mr-1 animate-spin" />
            ) : (
              <Wrench className="h-3 w-3 mr-1" />
            )}
            {maintenance.isPending
              ? t("settings.advanced.retention.running")
              : t("settings.advanced.retention.runNow")}
          </Button>
        </div>

        {statsQuery.isLoading ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground py-2">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("common.loading")}
          </div>
        ) : statsQuery.isError || !stats ? (
          <p className="text-sm text-destructive py-2">
            {extractErrorMessage(statsQuery.error) ||
              t("settings.advanced.retention.loadFailed")}
          </p>
        ) : (
          <>
            <p className="text-sm" data-testid="db-size">
              {t("settings.advanced.retention.dbSize", {
                size: formatBytes(stats.sizeBytes),
                free: formatBytes(stats.freeBytes),
              })}
            </p>
            <div className="max-h-64 overflow-y-auto rounded-md border border-border/50">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>
                      {t("settings.advanced.retention.table")}
                    </TableHead>
                    <TableHead className="text-right">
                      {t("settings.advanced.retention.rows")}
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {stats.tables.map((table) => (
                    <TableRow key={table.name}>
                      <TableCell className="font-mono text-xs">
                        {table.name}
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {numberFormat.format(table.rows)}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          </>
        )}
        <p className="text-xs text-muted-foreground">
          {t("settings.advanced.retention.maintenanceHint")}
        </p>
      </div>
    </div>
  );
}
