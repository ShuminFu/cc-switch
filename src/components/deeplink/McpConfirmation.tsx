import { useId, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { DeepLinkImportRequest } from "../../lib/api/deeplink";
import { decodeBase64Utf8 } from "../../lib/utils/base64";
import { Checkbox } from "@/components/ui/checkbox";
import {
  decodeBase64Lenient,
  formatCommandLine,
  isRiskyCommand,
  maskIfSecret,
} from "./safety";

type McpSpec = Record<string, unknown>;

function KeyValueList({
  label,
  entries,
}: {
  label: string;
  entries: Record<string, unknown>;
}) {
  const items = Object.entries(entries);
  if (items.length === 0) return null;
  return (
    <div className="mt-1">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-0.5 space-y-0.5 pl-2">
        {items.map(([key, value]) => (
          <div key={key} className="text-xs font-mono break-all">
            <span className="text-muted-foreground">{key}=</span>
            {maskIfSecret(key, value)}
          </div>
        ))}
      </div>
    </div>
  );
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

export function McpConfirmation({
  request,
  enableNow = false,
  onEnableNowChange,
}: {
  request: DeepLinkImportRequest;
  /** User's explicit choice to write the servers into the target apps now. */
  enableNow?: boolean;
  onEnableNowChange?: (value: boolean) => void;
}) {
  const { t } = useTranslation();
  const checkboxId = useId();

  const mcpServers = useMemo((): Record<string, McpSpec> | null => {
    if (!request.config) return null;
    try {
      const decoded =
        decodeBase64Lenient(request.config) ?? decodeBase64Utf8(request.config);
      const parsed = JSON.parse(decoded);
      return parsed.mcpServers || {};
    } catch (e) {
      console.error("Failed to parse MCP config:", e);
      return null;
    }
  }, [request.config]);

  const targetApps = (request.apps?.split(",") || [])
    .map((app) => app.trim())
    .filter(Boolean);
  const serverCount = Object.keys(mcpServers || {}).length;

  return (
    <div className="space-y-4">
      <h3 className="text-lg font-semibold">{t("deeplink.mcp.title")}</h3>

      <div className="rounded-lg border border-red-300 bg-red-50 p-3 text-sm text-red-800 dark:border-red-800 dark:bg-red-900/20 dark:text-red-200">
        {t("deeplink.mcp.executionWarning", {
          defaultValue:
            "MCP servers run commands on your computer with your user permissions whenever the target app starts. Review every command, argument and environment variable below and only import servers from sources you trust.",
        })}
      </div>

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
              {app}
            </span>
          ))}
        </div>
      </div>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          {t("deeplink.mcp.serverCount", { count: serverCount })}
        </label>
        <div className="mt-1 space-y-2 max-h-80 overflow-auto border rounded p-2 bg-muted/30">
          {mcpServers === null && (
            <div className="text-xs text-red-600">
              {t("deeplink.mcp.parseFailed", {
                defaultValue: "The MCP configuration could not be decoded.",
              })}
            </div>
          )}
          {mcpServers &&
            Object.entries(mcpServers).map(([id, rawSpec]) => {
              const spec = asRecord(rawSpec) ?? {};
              const type = typeof spec.type === "string" ? spec.type : "stdio";
              const isStdio = type === "stdio";
              const env = asRecord(spec.env);
              const headers = asRecord(spec.headers);
              const risky = isStdio && isRiskyCommand(spec.command, spec.args);
              return (
                <div
                  key={id}
                  className="p-2 bg-background rounded border"
                  data-testid={`mcp-server-${id}`}
                >
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-sm break-all">
                      {id}
                    </span>
                    <span className="px-1.5 py-0.5 rounded bg-muted text-[10px] uppercase text-muted-foreground">
                      {type}
                    </span>
                  </div>
                  {isStdio ? (
                    <>
                      <div className="mt-1 text-xs text-muted-foreground">
                        {t("deeplink.mcp.command", {
                          defaultValue: "Command",
                        })}
                      </div>
                      <pre className="mt-0.5 text-xs font-mono whitespace-pre-wrap break-all bg-muted/50 rounded p-1.5">
                        {formatCommandLine(spec.command, spec.args)}
                      </pre>
                      {typeof spec.cwd === "string" && (
                        <div className="mt-1 text-xs font-mono break-all">
                          <span className="text-muted-foreground">cwd: </span>
                          {spec.cwd}
                        </div>
                      )}
                      {env && (
                        <KeyValueList
                          label={t("deeplink.mcp.env", {
                            defaultValue: "Environment variables",
                          })}
                          entries={env}
                        />
                      )}
                    </>
                  ) : (
                    <>
                      <div className="mt-1 text-xs font-mono break-all">
                        <span className="text-muted-foreground">URL: </span>
                        {String(spec.url ?? "")}
                      </div>
                      {headers && (
                        <KeyValueList
                          label={t("deeplink.mcp.headers", {
                            defaultValue: "Headers",
                          })}
                          entries={headers}
                        />
                      )}
                    </>
                  )}
                  {risky && (
                    <div className="mt-1 text-xs font-medium text-red-600 dark:text-red-400">
                      {t("deeplink.mcp.riskyCommand", {
                        defaultValue:
                          "This server runs a shell or downloader, or its arguments contain shell metacharacters. It can execute arbitrary code.",
                      })}
                    </div>
                  )}
                </div>
              );
            })}
        </div>
      </div>

      <div className="space-y-2 rounded-lg border p-3">
        <div className="flex items-start gap-2">
          <Checkbox
            id={checkboxId}
            checked={enableNow}
            onCheckedChange={(value) => onEnableNowChange?.(value === true)}
          />
          <label htmlFor={checkboxId} className="text-sm leading-tight">
            {t("deeplink.mcp.enableNowLabel", {
              defaultValue:
                "Enable for the target apps now (write to their config files)",
            })}
          </label>
        </div>
        {request.enabled && (
          <div className="text-xs text-muted-foreground">
            {t("deeplink.mcp.linkRequestedEnable", {
              defaultValue:
                "The link asked to enable these servers immediately. They are only enabled if you tick the box above.",
            })}
          </div>
        )}
        {enableNow ? (
          <div className="text-yellow-600 dark:text-yellow-500 text-sm flex items-center gap-2">
            <span>⚠️</span>
            <span>{t("deeplink.mcp.enabledWarning")}</span>
          </div>
        ) : (
          <div className="text-xs text-muted-foreground">
            {t("deeplink.mcp.disabledHint", {
              defaultValue:
                "Servers will be saved to CC Switch disabled. No app config files are changed; you can enable them per app on the MCP page.",
            })}
          </div>
        )}
      </div>
    </div>
  );
}
