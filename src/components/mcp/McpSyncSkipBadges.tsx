import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { isAppId, APP_REGISTRY } from "@/config/apps";
import type { McpSyncSkip } from "@/lib/bindings";

interface McpSyncSkipBadgesProps {
  skips: McpSyncSkip[] | undefined;
}

/**
 * One warning badge per MCP app whose config directory is missing: MCP sync
 * writes nothing there until the tool has been run once (CFG-46).
 */
export function McpSyncSkipBadges({ skips }: McpSyncSkipBadgesProps) {
  const { t } = useTranslation();
  if (!skips || skips.length === 0) return null;

  return (
    <div
      className="mb-4 flex flex-wrap items-center gap-2"
      data-testid="mcp-sync-skips"
    >
      {skips.map((skip) => {
        const label = isAppId(skip.app)
          ? APP_REGISTRY[skip.app].label
          : skip.app;
        const hint = t("mcp.syncSkip.hint", {
          defaultValue:
            "{{app}} is not synced: its config directory {{dir}} does not exist. Run {{app}} once, then toggle the server again.",
          app: label,
          dir: skip.missingDir,
          interpolation: { escapeValue: false },
        });
        return (
          <span
            key={skip.app}
            role="status"
            title={hint}
            aria-label={hint}
            className="inline-flex items-center gap-1 rounded-full bg-amber-500/10 px-2.5 py-1 text-xs text-amber-700 dark:text-amber-300"
          >
            <AlertTriangle className="h-3.5 w-3.5" />
            {t("mcp.syncSkip.badge", {
              defaultValue: "{{app}}: not synced",
              app: label,
            })}
          </span>
        );
      })}
    </div>
  );
}
