import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import { usageKeys } from "@/lib/query/usage";
import {
  Database,
  FileText,
  RefreshCw,
  Loader2,
  History,
  AlertTriangle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { listen } from "@tauri-apps/api/event";
import type { TFunction } from "i18next";
import type { TranslationKey } from "@/i18n/keys";
import type { DataSourceSummary, SessionRescanProgress } from "@/types/usage";

/** 后端重扫进度事件（载荷见 `SessionRescanProgress`） */
const RESCAN_PROGRESS_EVENT = "session-usage-rescan-progress";

interface DataSourceBarProps {
  refreshIntervalMs: number;
}

const DATA_SOURCE_ICONS: Record<string, React.ReactNode> = {
  proxy: <Database className="h-3.5 w-3.5" />,
  session_log: <FileText className="h-3.5 w-3.5" />,
  codex_db: <Database className="h-3.5 w-3.5" />,
  codex_session: <FileText className="h-3.5 w-3.5" />,
  gemini_session: <FileText className="h-3.5 w-3.5" />,
  opencode_session: <FileText className="h-3.5 w-3.5" />,
};

const DATA_SOURCE_LABEL_KEYS: Record<string, TranslationKey> = {
  proxy: "usage.dataSource.proxy",
  session_log: "usage.dataSource.session_log",
  codex_db: "usage.dataSource.codex_db",
  codex_session: "usage.dataSource.codex_session",
  gemini_session: "usage.dataSource.gemini_session",
  opencode_session: "usage.dataSource.opencode_session",
};

function formatDate(ts: number | null): string {
  return ts == null ? "—" : new Date(ts * 1000).toLocaleDateString();
}

/** 每个来源的数据质量摘要（悬浮提示） */
function qualityTooltip(source: DataSourceSummary, t: TFunction): string {
  return [
    t("usage.dataQuality.title"),
    t("usage.dataQuality.duplicates", { count: source.duplicateCount }),
    t("usage.dataQuality.unpriced", { count: source.unpricedCount }),
    t("usage.dataQuality.missingSession", {
      count: source.missingSessionCount,
    }),
    t("usage.dataQuality.zeroOutput", { count: source.zeroOutputCount }),
    t("usage.dataQuality.range", {
      from: formatDate(source.firstCreatedAt),
      to: formatDate(source.lastCreatedAt),
    }),
  ].join("\n");
}

function hasQualityIssues(source: DataSourceSummary): boolean {
  return source.unpricedCount > 0 || source.zeroOutputCount > 0;
}

export function DataSourceBar({ refreshIntervalMs }: DataSourceBarProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [syncing, setSyncing] = useState(false);
  const [rescanProgress, setRescanProgress] =
    useState<SessionRescanProgress | null>(null);
  const [rescanning, setRescanning] = useState(false);

  useEffect(() => {
    if (!rescanning) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<SessionRescanProgress>(RESCAN_PROGRESS_EVENT, (event) => {
      setRescanProgress(event.payload);
    })
      .then((off) => {
        if (disposed) off();
        else unlisten = off;
      })
      .catch(() => {
        // 非 Tauri 环境（测试 / 浏览器预览）没有事件通道，只显示转圈
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [rescanning]);

  const { data: sources } = useQuery({
    queryKey: [...usageKeys.all, "data-sources"],
    queryFn: usageApi.getDataSourceBreakdown,
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    refetchIntervalInBackground: false,
  });

  const handleSync = async () => {
    setSyncing(true);
    try {
      const result = await usageApi.syncSessionUsage();
      if (result.imported > 0) {
        toast.success(
          t("usage.sessionSync.imported", {
            count: result.imported,
          }),
        );
        // Refresh all usage data
        queryClient.invalidateQueries({ queryKey: usageKeys.all });
      } else {
        toast.info(t("usage.sessionSync.upToDate"));
      }
    } catch {
      toast.error(t("usage.sessionSync.failed"));
    } finally {
      setSyncing(false);
    }
  };

  const handleRescan = async () => {
    setRescanning(true);
    setRescanProgress(null);
    try {
      const result = await usageApi.rescanSessionUsage();
      toast.success(
        t("usage.sessionSync.rescanDone", { count: result.imported }),
      );
      queryClient.invalidateQueries({ queryKey: usageKeys.all });
    } catch {
      toast.error(t("usage.sessionSync.rescanFailed"));
    } finally {
      setRescanning(false);
      setRescanProgress(null);
    }
  };

  const busy = syncing || rescanning;

  if (!sources || sources.length === 0) {
    return null;
  }

  const hasNonProxy = sources.some((s) => s.dataSource !== "proxy");

  return (
    <div className="flex items-center gap-3 text-xs text-muted-foreground bg-muted/30 rounded-lg px-4 py-2">
      <span className="font-medium text-foreground/70">
        {t("usage.dataSources")}:
      </span>
      <div className="flex items-center gap-3 flex-wrap">
        {sources.map((source) => (
          <div
            key={source.dataSource}
            className="flex items-center gap-1.5 bg-background/50 rounded-md px-2 py-1"
            title={qualityTooltip(source, t)}
          >
            {DATA_SOURCE_ICONS[source.dataSource] ?? (
              <Database className="h-3.5 w-3.5" />
            )}
            <span>
              {source.dataSource in DATA_SOURCE_LABEL_KEYS
                ? t(DATA_SOURCE_LABEL_KEYS[source.dataSource])
                : source.dataSource}
            </span>
            <span className="font-mono font-medium text-foreground/80">
              {source.requestCount.toLocaleString()}
            </span>
            {hasQualityIssues(source) && (
              <AlertTriangle
                className="h-3 w-3 text-amber-500"
                aria-label={t("usage.dataQuality.title")}
              />
            )}
          </div>
        ))}
      </div>

      <div className="ml-auto flex items-center gap-1">
        {rescanning && rescanProgress && !rescanProgress.done && (
          <span className="text-xs">
            {t("usage.sessionSync.rescanProgress", {
              source: rescanProgress.source,
              completed: rescanProgress.completed,
              total: rescanProgress.total,
            })}
          </span>
        )}
        {hasNonProxy && (
          <Button
            variant="ghost"
            size="sm"
            className="h-7 px-2 text-xs"
            onClick={handleRescan}
            disabled={busy}
            title={t("usage.sessionSync.rescanTitle")}
          >
            {rescanning ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <History className="h-3.5 w-3.5" />
            )}
            <span className="ml-1">{t("usage.sessionSync.rescan")}</span>
          </Button>
        )}
        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-xs"
          onClick={handleSync}
          disabled={busy}
          title={t("usage.sessionSync.trigger")}
        >
          {syncing ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" />
          )}
          <span className="ml-1">
            {hasNonProxy
              ? t("usage.sessionSync.resync")
              : t("usage.sessionSync.import")}
          </span>
        </Button>
      </div>
    </div>
  );
}
