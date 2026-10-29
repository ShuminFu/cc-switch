import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  DeepLinkImportRequest,
  DeepLinkPreviewMcpServer,
} from "../../lib/api/deeplink";
import { decodeBase64Utf8 } from "../../lib/utils/base64";
import { maskSecret, maskValue } from "../../lib/deeplink/mask";

/** Servers of an MCP link, parsed client-side when no preview is available. */
export function parseMcpServers(
  request: DeepLinkImportRequest,
): DeepLinkPreviewMcpServer[] | null {
  if (!request.config) return null;
  try {
    const parsed = JSON.parse(decodeBase64Utf8(request.config));
    const servers = (parsed.mcpServers || {}) as Record<
      string,
      Record<string, unknown>
    >;
    return Object.entries(servers).map(([id, spec]) => ({
      id,
      command: typeof spec.command === "string" ? spec.command : undefined,
      args: Array.isArray(spec.args) ? spec.args.map((a) => String(a)) : [],
      url: typeof spec.url === "string" ? spec.url : undefined,
      spec,
      exists: false,
    }));
  } catch (e) {
    console.error("Failed to parse MCP config:", e);
    return null;
  }
}

const asStringMap = (value: unknown): Array<[string, string]> =>
  value && typeof value === "object" && !Array.isArray(value)
    ? Object.entries(value as Record<string, unknown>).map(([k, v]) => [
        k,
        typeof v === "string" ? v : JSON.stringify(v),
      ])
    : [];

export function McpConfirmation({
  request,
  servers: previewServers,
}: {
  request: DeepLinkImportRequest;
  /** Servers from the backend preview (adds the "already installed" flag) */
  servers?: DeepLinkPreviewMcpServer[];
}) {
  const { t } = useTranslation();

  const servers = useMemo(
    () => previewServers ?? parseMcpServers(request),
    [previewServers, request],
  );

  const targetApps = request.apps?.split(",") || [];
  const serverCount = servers?.length ?? 0;

  return (
    <div className="space-y-4">
      <h3 className="text-lg font-semibold">{t("deeplink.mcp.title")}</h3>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          {t("deeplink.mcp.targetApps")}
        </label>
        <div className="mt-1 flex gap-2 flex-wrap">
          {targetApps.map((app) => (
            <span
              key={app}
              className="px-2 py-1 bg-primary/10 text-primary text-xs rounded capitalize"
            >
              {app.trim()}
            </span>
          ))}
        </div>
      </div>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          {t("deeplink.mcp.serverCount", { count: serverCount })}
        </label>
        <div className="mt-1 space-y-2 border rounded p-2 bg-muted/30">
          {servers?.map((server) => {
            const risky = !!server.command && !server.exists;
            const env = asStringMap(server.spec.env);
            const headers = asStringMap(server.spec.headers);
            return (
              <div
                key={server.id}
                data-testid={`deeplink-mcp-server-${server.id}`}
                className={`p-2 rounded border text-xs space-y-1 ${
                  risky
                    ? "border-red-300 dark:border-red-800 bg-red-50 dark:bg-red-900/20"
                    : "bg-background"
                }`}
              >
                <div className="font-semibold text-sm">{server.id}</div>
                {server.command ? (
                  <div className="space-y-0.5">
                    <div className="text-muted-foreground">
                      {t("deeplink.mcp.command")}
                    </div>
                    <div className="font-mono break-all whitespace-pre-wrap">
                      {[server.command, ...server.args].join(" ")}
                    </div>
                  </div>
                ) : (
                  <div className="space-y-0.5">
                    <div className="text-muted-foreground">
                      {t("deeplink.mcp.url")}
                    </div>
                    <div className="font-mono break-all">{server.url}</div>
                  </div>
                )}
                {env.length > 0 && (
                  <div className="space-y-0.5">
                    <div className="text-muted-foreground">
                      {t("deeplink.mcp.env")}
                    </div>
                    {env.map(([key, value]) => (
                      <div key={key} className="font-mono break-all">
                        {key}={maskValue(key, value)}
                      </div>
                    ))}
                  </div>
                )}
                {headers.length > 0 && (
                  <div className="space-y-0.5">
                    <div className="text-muted-foreground">
                      {t("deeplink.mcp.headers")}
                    </div>
                    {/* Header values (Authorization, ...) are always masked */}
                    {headers.map(([key, value]) => (
                      <div key={key} className="font-mono break-all">
                        {key}: {maskSecret(value)}
                      </div>
                    ))}
                  </div>
                )}
                {risky && (
                  <div className="font-medium text-red-700 dark:text-red-300">
                    {t("deeplink.mcp.commandWarning")}
                  </div>
                )}
                {server.exists && (
                  <div className="text-muted-foreground">
                    {t("deeplink.mcp.existsNote")}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>

      {request.enabled && (
        <div className="text-yellow-600 dark:text-yellow-500 text-sm flex items-center gap-2">
          <span>⚠️</span>
          <span>{t("deeplink.mcp.enabledWarning")}</span>
        </div>
      )}
    </div>
  );
}
