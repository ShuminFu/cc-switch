import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import { usageKeys } from "@/lib/query/usage";
import { Database, FileText, RefreshCw, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useState } from "react";
import { toast } from "sonner";
import type { TranslationKey } from "@/i18n/keys";

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

export function DataSourceBar({ refreshIntervalMs }: DataSourceBarProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [syncing, setSyncing] = useState(false);

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
          </div>
        ))}
      </div>

      <div className="ml-auto">
        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-xs"
          onClick={handleSync}
          disabled={syncing}
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
