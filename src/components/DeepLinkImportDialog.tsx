import { useState, useEffect, useMemo, useRef, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  DeepLinkImportPreview,
  DeepLinkImportRequest,
  DeepLinkPreviewRisk,
  deeplinkApi,
} from "@/lib/api/deeplink";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { PromptConfirmation } from "./deeplink/PromptConfirmation";
import { McpConfirmation, parseMcpServers } from "./deeplink/McpConfirmation";
import {
  ExistingProviderDiff,
  LiveFilesPreview,
  RiskAcknowledgement,
  UsageScriptPreview,
} from "./deeplink/ImportPreviewSections";
import { SkillConfirmation } from "./deeplink/SkillConfirmation";
import { ProviderIcon } from "./ProviderIcon";
import { invalidateProviderGraph } from "@/entities/provider/invalidate";
import { providerKeys } from "@/entities/provider/keys";
import { classifyClaudeEnvKey } from "@/lib/deeplink/envKeys";
import { maskSecret, maskValue } from "@/lib/deeplink/mask";
import { isAppId } from "@/entities/app/registry";
import type { AppId } from "@/lib/api/types";

interface DeeplinkError {
  url: string;
  error: string;
}

/** Shape of the embedded provider config: env vars, or Codex auth + TOML */
type ConfigKind = "claudeEnv" | "codexToml" | "geminiEnv";

interface DeeplinkAppFormat {
  config: ConfigKind;
  /** Separate Haiku / Sonnet / Opus model fields instead of a single model */
  tieredModels: boolean;
}

/** Provider deep-link formats per app; apps not listed have no config preview */
const DEEPLINK_APP_FORMATS: Partial<Record<AppId, DeeplinkAppFormat>> = {
  claude: { config: "claudeEnv", tieredModels: true },
  codex: { config: "codexToml", tieredModels: false },
  gemini: { config: "geminiEnv", tieredModels: false },
};

const deeplinkAppFormat = (app: unknown): DeeplinkAppFormat | undefined =>
  isAppId(app) ? DEEPLINK_APP_FORMATS[app] : undefined;

export function DeepLinkImportDialog() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [request, setRequest] = useState<DeepLinkImportRequest | null>(null);
  const [preview, setPreview] = useState<DeepLinkImportPreview | null>(null);
  // Extra confirmation for links carrying risky items (see `risks` below)
  const [riskAcknowledged, setRiskAcknowledged] = useState(false);
  // A newer link arrived (and was ignored) while this dialog was open
  const [newerLinkIgnored, setNewerLinkIgnored] = useState(false);
  const [isImporting, setIsImporting] = useState(false);
  const [isOpen, setIsOpenState] = useState(false);
  // Refs mirror the dialog state for the event listener, which is registered
  // once and would otherwise read stale values.
  const isOpenRef = useRef(false);
  const isImportingRef = useRef(false);
  // True while a request's config is being merged (before the dialog opens)
  const isMergingRef = useRef(false);
  // Bumped for every accepted request; a merge result is applied only if it
  // still belongs to the latest one (e.g. a slow remote configUrl fetch).
  const requestSeqRef = useRef(0);

  const setIsOpen = useCallback((open: boolean) => {
    isOpenRef.current = open;
    setIsOpenState(open);
  }, []);

  // 容错判断：MCP 导入结果可能缺少 type 字段
  const isMcpImportResult = (
    value: unknown,
  ): value is {
    importedCount: number;
    importedIds: string[];
    failed: Array<{ id: string; error: string }>;
    type?: "mcp";
  } => {
    if (!value || typeof value !== "object") return false;
    const v = value as Record<string, unknown>;
    return (
      typeof v.importedCount === "number" &&
      Array.isArray(v.importedIds) &&
      Array.isArray(v.failed)
    );
  };

  useEffect(() => {
    // Same ref object as requestSeqRef; held locally so the cleanup bumps the
    // counter this listener compares against.
    const requestSeq = requestSeqRef;
    // Listen for deep link import events
    const unlistenImport = listen<DeepLinkImportRequest>(
      "deeplink-import",
      async (event) => {
        // Never swap the request under an open (or about to open) confirm
        // dialog: the user could end up confirming content they never saw.
        if (
          isOpenRef.current ||
          isImportingRef.current ||
          isMergingRef.current
        ) {
          toast.warning(t("deeplink.requestIgnoredBusy"), {
            description: t("deeplink.requestIgnoredBusyDescription"),
          });
          if (isOpenRef.current) setNewerLinkIgnored(true);
          return;
        }

        const seq = ++requestSeq.current;
        let nextRequest: DeepLinkImportRequest = event.payload;
        let nextPreview: DeepLinkImportPreview | null = null;
        const hasConfig = !!(event.payload.config || event.payload.configUrl);
        const wantsPreview =
          event.payload.resource === "provider" ||
          event.payload.resource === "mcp" ||
          !event.payload.resource;

        if (wantsPreview) {
          // The preview merges the config like mergeDeeplinkConfig and adds
          // the risk details shown in the dialog.
          isMergingRef.current = true;
          try {
            nextPreview = await deeplinkApi.previewDeeplinkImport(
              event.payload,
            );
            nextRequest = nextPreview.request;
          } catch (error) {
            console.error("Failed to build import preview:", error);
            if (hasConfig) {
              toast.error(t("deeplink.configMergeError"), {
                description:
                  error instanceof Error ? error.message : String(error),
              });
            }
            // Fall back to the original request; risks are derived locally
            nextRequest = event.payload;
          } finally {
            isMergingRef.current = false;
          }
        } else if (hasConfig) {
          // If config is present, merge it to get the complete configuration
          isMergingRef.current = true;
          try {
            nextRequest = await deeplinkApi.mergeDeeplinkConfig(event.payload);
          } catch (error) {
            console.error("Failed to merge config:", error);
            toast.error(t("deeplink.configMergeError"), {
              description:
                error instanceof Error ? error.message : String(error),
            });
            // Fall back to original request
            nextRequest = event.payload;
          } finally {
            isMergingRef.current = false;
          }
        }

        // A newer request (or unmount) superseded this one while merging
        if (seq !== requestSeq.current) return;

        setRequest(nextRequest);
        setPreview(nextPreview);
        setRiskAcknowledged(false);
        setNewerLinkIgnored(false);
        setIsOpen(true);
      },
    );

    // Listen for deep link error events
    const unlistenError = listen<DeeplinkError>("deeplink-error", (event) => {
      // Only log the error message: the payload URL may carry secrets
      console.error("Deep link error:", event.payload.error);
      toast.error(t("deeplink.parseError"), {
        description: event.payload.error,
      });
    });

    return () => {
      // Drop any merge still in flight for this listener
      requestSeq.current++;
      unlistenImport.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
  }, [t, setIsOpen]);

  const handleImport = async () => {
    if (!request || isImportingRef.current) return;
    if (risks.length > 0 && !riskAcknowledged) return;

    isImportingRef.current = true;
    setIsImporting(true);

    try {
      const result = await deeplinkApi.importFromDeeplink(request);
      const refreshMcp = async (summary: {
        importedCount: number;
        importedIds: string[];
        failed: Array<{ id: string; error: string }>;
      }) => {
        // 强制刷新 MCP 相关缓存，确保管理页重新从数据库加载
        await queryClient.invalidateQueries({
          queryKey: ["mcp", "all"],
          refetchType: "all",
        });
        await queryClient.refetchQueries({
          queryKey: ["mcp", "all"],
          type: "all",
        });

        if (summary.failed.length > 0) {
          toast.warning(t("deeplink.mcpPartialSuccess"), {
            description: t("deeplink.mcpPartialSuccessDescription", {
              success: summary.importedCount,
              failed: summary.failed.length,
            }),
          });
        } else {
          toast.success(t("deeplink.mcpImportSuccess"), {
            description: t("deeplink.mcpImportSuccessDescription", {
              count: summary.importedCount,
            }),
            closeButton: true,
          });
        }
      };

      const refreshProviders = async () => {
        if (request.app) {
          await invalidateProviderGraph(queryClient, request.app);
        } else {
          await queryClient.invalidateQueries({ queryKey: providerKeys.all });
        }
      };

      // Handle different result types
      if ("type" in result) {
        if (result.type === "provider") {
          await refreshProviders();
          toast.success(t("deeplink.importSuccess"), {
            description: t("deeplink.importSuccessDescription", {
              name: request.name,
            }),
            closeButton: true,
          });
        } else if (result.type === "prompt") {
          // Prompts don't use React Query, trigger a custom event for refresh
          window.dispatchEvent(
            new CustomEvent("prompt-imported", {
              detail: { app: request.app },
            }),
          );
          toast.success(t("deeplink.promptImportSuccess"), {
            description: t("deeplink.promptImportSuccessDescription", {
              name: request.name,
            }),
            closeButton: true,
          });
        } else if (result.type === "mcp") {
          await refreshMcp(result);
        } else if (result.type === "skill") {
          // Refresh Skills with aggressive strategy
          queryClient.invalidateQueries({
            queryKey: ["skills"],
            refetchType: "all",
          });
          await queryClient.refetchQueries({
            queryKey: ["skills"],
            type: "all",
          });
          toast.success(t("deeplink.skillImportSuccess"), {
            description: t("deeplink.skillImportSuccessDescription", {
              repo: request.repo,
            }),
            closeButton: true,
          });
        }
      } else if (isMcpImportResult(result)) {
        // 兜底处理：旧版本后端可能未返回 type 字段
        await refreshMcp(result);
      } else {
        // Legacy return type (string ID) - assume provider
        await refreshProviders();
        toast.success(t("deeplink.importSuccess"), {
          description: t("deeplink.importSuccessDescription", {
            name: request.name,
          }),
          closeButton: true,
        });
      }

      // Close dialog after all refreshes complete
      setIsOpen(false);
    } catch (error) {
      console.error("Failed to import from deep link:", error);
      toast.error(t("deeplink.importError"), {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      isImportingRef.current = false;
      setIsImporting(false);
    }
  };

  const handleCancel = () => {
    setIsOpen(false);
  };

  const maskedApiKey = maskSecret(request?.apiKey);

  // Check if config file is present
  const hasConfigFile = !!(request?.config || request?.configUrl);
  const configSource = request?.config
    ? "base64"
    : request?.configUrl
      ? "url"
      : null;

  // Parse config file content for display
  interface ParsedConfig {
    type: ConfigKind;
    env?: Record<string, string>;
    auth?: Record<string, string>;
    tomlConfig?: string;
    raw: Record<string, unknown>;
  }

  // Helper to decode base64 with UTF-8 support
  const b64ToUtf8 = (str: string): string => {
    try {
      const binString = atob(str);
      const bytes = Uint8Array.from(binString, (m) => m.codePointAt(0) || 0);
      return new TextDecoder().decode(bytes);
    } catch (e) {
      console.error("Failed to decode base64:", e);
      return atob(str);
    }
  };

  const parsedConfig = useMemo((): ParsedConfig | null => {
    if (!request?.config) return null;
    try {
      const decoded = b64ToUtf8(request.config);
      const parsed = JSON.parse(decoded) as Record<string, unknown>;

      const format = deeplinkAppFormat(request.app)?.config;
      if (format === "claudeEnv") {
        // Claude 格式: { env: { ANTHROPIC_AUTH_TOKEN: ..., ... } }
        return {
          type: "claudeEnv",
          env: (parsed.env as Record<string, string>) || {},
          raw: parsed,
        };
      } else if (format === "codexToml") {
        // Codex 格式: { auth: { OPENAI_API_KEY: ... }, config: "TOML string" }
        return {
          type: "codexToml",
          auth: (parsed.auth as Record<string, string>) || {},
          tomlConfig: (parsed.config as string) || "",
          raw: parsed,
        };
      } else if (format === "geminiEnv") {
        // Gemini 格式: 扁平结构 { GEMINI_API_KEY: ..., GEMINI_BASE_URL: ... }
        return {
          type: "geminiEnv",
          env: parsed as Record<string, string>,
          raw: parsed,
        };
      }
      return null;
    } catch (e) {
      console.error("Failed to parse config:", e);
      return null;
    }
  }, [request?.config, request?.app]);

  // Split Claude env keys by how the backend treats them on import
  const claudeEnvGroups = useMemo(() => {
    const groups = {
      allowed: [] as Array<[string, string]>,
      denied: [] as Array<[string, string]>,
      unlisted: [] as Array<[string, string]>,
    };
    if (parsedConfig?.type !== "claudeEnv" || !parsedConfig.env) return groups;
    for (const [key, value] of Object.entries(parsedConfig.env)) {
      groups[classifyClaudeEnvKey(key)].push([key, String(value)]);
    }
    return groups;
  }, [parsedConfig]);

  // Usage script source: from the preview, or decoded locally as a fallback
  const usageScriptCode = useMemo(() => {
    if (preview?.usageScript) return preview.usageScript.code;
    if (!request?.usageScript) return null;
    try {
      return b64ToUtf8(request.usageScript);
    } catch {
      return request.usageScript;
    }
  }, [preview, request?.usageScript]);

  // Items that need the extra confirmation. The backend preview is
  // authoritative; local checks keep the gate when the preview failed.
  const risks = useMemo((): DeepLinkPreviewRisk[] => {
    const found = new Set<DeepLinkPreviewRisk>(preview?.risks ?? []);
    if (!request) return [];
    if (claudeEnvGroups.unlisted.length > 0) found.add("unlistedEnv");
    if (usageScriptCode?.trim()) found.add("usageScript");
    if (
      !preview &&
      request.resource === "mcp" &&
      parseMcpServers(request)?.some((s) => !!s.command)
    ) {
      found.add("mcpCommand");
    }
    return Array.from(found);
  }, [preview, request, claudeEnvGroups, usageScriptCode]);
  const importBlocked = risks.length > 0 && !riskAcknowledged;

  const getTitle = () => {
    if (!request) return t("deeplink.confirmImport");
    switch (request.resource) {
      case "prompt":
        return t("deeplink.importPrompt");
      case "mcp":
        return t("deeplink.importMcp");
      case "skill":
        return t("deeplink.importSkill");
      default:
        return t("deeplink.confirmImport");
    }
  };

  const getDescription = () => {
    if (!request) return t("deeplink.confirmImportDescription");
    switch (request.resource) {
      case "prompt":
        return t("deeplink.importPromptDescription");
      case "mcp":
        return t("deeplink.importMcpDescription");
      case "skill":
        return t("deeplink.importSkillDescription");
      default:
        return t("deeplink.confirmImportDescription");
    }
  };

  return (
    <Dialog open={isOpen && !!request} onOpenChange={setIsOpen}>
      <DialogContent className="sm:max-w-[500px]" zIndex="top">
        {request && (
          <>
            {/* 标题显式左对齐，避免默认居中样式影响 */}
            <DialogHeader className="text-left sm:text-left">
              <DialogTitle>{getTitle()}</DialogTitle>
              <DialogDescription>{getDescription()}</DialogDescription>
            </DialogHeader>

            {/* 主体内容整体右移，略大于标题内边距，让内容看起来不贴边 */}
            <div className="space-y-4 px-8 py-4 max-h-[60vh] overflow-y-auto [scrollbar-width:thin] [&::-webkit-scrollbar]:w-1.5 [&::-webkit-scrollbar]:block [&::-webkit-scrollbar-thumb]:rounded-full [&::-webkit-scrollbar-thumb]:bg-gray-200 dark:[&::-webkit-scrollbar-thumb]:bg-gray-700">
              {request.resource === "prompt" && (
                <PromptConfirmation request={request} />
              )}
              {newerLinkIgnored && (
                <div
                  role="status"
                  data-testid="deeplink-newer-link-banner"
                  className="rounded-lg border border-yellow-300 dark:border-yellow-800 bg-yellow-50 dark:bg-yellow-900/20 p-3 text-xs text-yellow-800 dark:text-yellow-200"
                >
                  {t("deeplink.preview.newerLinkIgnored")}
                </div>
              )}

              {request.resource === "mcp" && (
                <>
                  <McpConfirmation
                    request={request}
                    servers={preview?.mcpServers}
                  />
                  {preview && <LiveFilesPreview preview={preview} />}
                </>
              )}
              {request.resource === "skill" && (
                <SkillConfirmation request={request} />
              )}

              {/* Legacy Provider View */}
              {(request.resource === "provider" || !request.resource) && (
                <>
                  {/* Provider Icon - enlarge and center near the top */}
                  {request.icon && (
                    <div className="flex justify-center pt-2 pb-1">
                      <ProviderIcon
                        icon={request.icon}
                        name={request.name || request.icon}
                        size={80}
                        className="drop-shadow-sm"
                      />
                    </div>
                  )}

                  {/* App Type */}
                  <div className="grid grid-cols-3 items-center gap-4">
                    <div className="font-medium text-sm text-muted-foreground">
                      {t("deeplink.app")}
                    </div>
                    <div className="col-span-2 text-sm font-medium capitalize">
                      {request.app}
                    </div>
                  </div>

                  {/* Provider Name */}
                  <div className="grid grid-cols-3 items-center gap-4">
                    <div className="font-medium text-sm text-muted-foreground">
                      {t("deeplink.providerName")}
                    </div>
                    <div className="col-span-2 text-sm font-medium">
                      {request.name}
                    </div>
                  </div>

                  {/* Homepage */}
                  <div className="grid grid-cols-3 items-center gap-4">
                    <div className="font-medium text-sm text-muted-foreground">
                      {t("deeplink.homepage")}
                    </div>
                    <div className="col-span-2 text-sm break-all text-blue-600 dark:text-blue-400">
                      {request.homepage}
                    </div>
                  </div>

                  {/* API Endpoint */}
                  <div className="grid grid-cols-3 items-start gap-4">
                    <div className="font-medium text-sm text-muted-foreground pt-0.5">
                      {t("deeplink.endpoint")}
                    </div>
                    <div className="col-span-2 text-sm break-all space-y-1">
                      {request.endpoint?.split(",").map((ep, idx) => (
                        <div
                          key={idx}
                          className={
                            idx === 0 ? "font-medium" : "text-muted-foreground"
                          }
                        >
                          {idx === 0 ? "🔹 " : "└ "}
                          {ep.trim()}
                          {idx === 0 && request.endpoint?.includes(",") && (
                            <span className="text-xs text-muted-foreground ml-2">
                              ({t("deeplink.primaryEndpoint")})
                            </span>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>

                  {/* API Key (masked) */}
                  <div className="grid grid-cols-3 items-center gap-4">
                    <div className="font-medium text-sm text-muted-foreground">
                      {t("deeplink.apiKey")}
                    </div>
                    <div className="col-span-2 text-sm font-mono text-muted-foreground">
                      {maskedApiKey}
                    </div>
                  </div>

                  {/* Model Fields - 根据应用类型显示不同的模型字段 */}
                  {deeplinkAppFormat(request.app)?.tieredModels ? (
                    <>
                      {/* Claude 四种模型字段 */}
                      {request.haikuModel && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.haikuModel")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.haikuModel}
                          </div>
                        </div>
                      )}
                      {request.sonnetModel && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.sonnetModel")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.sonnetModel}
                          </div>
                        </div>
                      )}
                      {request.opusModel && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.opusModel")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.opusModel}
                          </div>
                        </div>
                      )}
                      {request.model && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.multiModel")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.model}
                          </div>
                        </div>
                      )}
                    </>
                  ) : (
                    <>
                      {/* Codex 和 Gemini 使用通用 model 字段 */}
                      {request.model && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.model")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.model}
                          </div>
                        </div>
                      )}
                    </>
                  )}

                  {/* Notes (if present) */}
                  {request.notes && (
                    <div className="grid grid-cols-3 items-start gap-4">
                      <div className="font-medium text-sm text-muted-foreground">
                        {t("deeplink.notes")}
                      </div>
                      <div className="col-span-2 text-sm text-muted-foreground">
                        {request.notes}
                      </div>
                    </div>
                  )}

                  {/* Config File Details (v3.8+) */}
                  {hasConfigFile && (
                    <div className="space-y-3 pt-2 border-t border-border-default">
                      <div className="grid grid-cols-3 items-center gap-4">
                        <div className="font-medium text-sm text-muted-foreground">
                          {t("deeplink.configSource")}
                        </div>
                        <div className="col-span-2 text-sm">
                          <span className="inline-flex items-center px-2 py-0.5 rounded-md bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 text-xs font-medium">
                            {configSource === "base64"
                              ? t("deeplink.configEmbedded")
                              : t("deeplink.configRemote")}
                          </span>
                          {request.configFormat && (
                            <span className="ml-2 text-xs text-muted-foreground uppercase">
                              {request.configFormat}
                            </span>
                          )}
                        </div>
                      </div>

                      {/* Parsed Config Details */}
                      {parsedConfig && (
                        <div className="rounded-lg bg-muted/50 p-3 space-y-2">
                          <div className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                            {t("deeplink.configDetails")}
                          </div>

                          {/* Claude config */}
                          {parsedConfig.type === "claudeEnv" &&
                            parsedConfig.env && (
                              <div className="space-y-1.5">
                                {claudeEnvGroups.allowed.map(([key, value]) => (
                                  <div
                                    key={key}
                                    className="grid grid-cols-2 gap-2 text-xs"
                                  >
                                    <span className="font-mono text-muted-foreground truncate">
                                      {key}
                                    </span>
                                    <span className="font-mono truncate">
                                      {maskValue(key, value)}
                                    </span>
                                  </div>
                                ))}
                              </div>
                            )}

                          {/* Codex config */}
                          {parsedConfig.type === "codexToml" && (
                            <div className="space-y-2">
                              {parsedConfig.auth &&
                                Object.keys(parsedConfig.auth).length > 0 && (
                                  <div className="space-y-1.5">
                                    <div className="text-xs text-muted-foreground">
                                      Auth:
                                    </div>
                                    {Object.entries(parsedConfig.auth).map(
                                      ([key, value]) => (
                                        <div
                                          key={key}
                                          className="grid grid-cols-2 gap-2 text-xs pl-2"
                                        >
                                          <span className="font-mono text-muted-foreground truncate">
                                            {key}
                                          </span>
                                          <span className="font-mono truncate">
                                            {maskValue(key, String(value))}
                                          </span>
                                        </div>
                                      ),
                                    )}
                                  </div>
                                )}
                              {parsedConfig.tomlConfig && (
                                <div className="space-y-1">
                                  <div className="text-xs text-muted-foreground">
                                    TOML Config:
                                  </div>
                                  <pre className="text-xs font-mono bg-background p-2 rounded overflow-x-auto max-h-24 whitespace-pre-wrap">
                                    {parsedConfig.tomlConfig.substring(0, 300)}
                                    {parsedConfig.tomlConfig.length > 300 &&
                                      "..."}
                                  </pre>
                                </div>
                              )}
                            </div>
                          )}

                          {/* Gemini config */}
                          {parsedConfig.type === "geminiEnv" &&
                            parsedConfig.env && (
                              <div className="space-y-1.5">
                                {Object.entries(parsedConfig.env).map(
                                  ([key, value]) => (
                                    <div
                                      key={key}
                                      className="grid grid-cols-2 gap-2 text-xs"
                                    >
                                      <span className="font-mono text-muted-foreground truncate">
                                        {key}
                                      </span>
                                      <span className="font-mono truncate">
                                        {maskValue(key, String(value))}
                                      </span>
                                    </div>
                                  ),
                                )}
                              </div>
                            )}
                        </div>
                      )}

                      {/* Non-allow-listed Claude env keys: full values, highlighted */}
                      {claudeEnvGroups.unlisted.length > 0 && (
                        <div
                          role="alert"
                          data-testid="deeplink-unlisted-env"
                          className="rounded-lg border border-red-300 dark:border-red-800 bg-red-50 dark:bg-red-900/20 p-3 space-y-2"
                        >
                          <div className="text-xs font-semibold text-red-700 dark:text-red-300">
                            {t("deeplink.unlistedEnvTitle")}
                          </div>
                          <div className="text-xs text-red-700 dark:text-red-300">
                            {t("deeplink.unlistedEnvDescription")}
                          </div>
                          {claudeEnvGroups.unlisted.map(([key, value]) => (
                            <div key={key} className="text-xs space-y-0.5">
                              <div className="font-mono font-medium break-all">
                                {key}
                              </div>
                              <div className="font-mono break-all whitespace-pre-wrap">
                                {value}
                              </div>
                            </div>
                          ))}
                          {request.enabled && (
                            <div className="text-xs font-medium text-red-700 dark:text-red-300">
                              {t("deeplink.unlistedEnvNoAutoSwitch")}
                            </div>
                          )}
                        </div>
                      )}

                      {/* Process-hijacking Claude env keys dropped by the backend */}
                      {claudeEnvGroups.denied.length > 0 && (
                        <div
                          data-testid="deeplink-denied-env"
                          className="rounded-lg border border-yellow-300 dark:border-yellow-800 bg-yellow-50 dark:bg-yellow-900/20 p-3 space-y-1 text-xs text-yellow-800 dark:text-yellow-200"
                        >
                          <div className="font-semibold">
                            {t("deeplink.deniedEnvTitle")}
                          </div>
                          <div className="font-mono break-all">
                            {claudeEnvGroups.denied
                              .map(([key]) => key)
                              .join(", ")}
                          </div>
                        </div>
                      )}

                      {/* Config URL (if remote) */}
                      {request.configUrl && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.configUrl")}
                          </div>
                          <div className="col-span-2 text-sm font-mono text-muted-foreground break-all">
                            {request.configUrl}
                          </div>
                        </div>
                      )}
                    </div>
                  )}

                  {/* Usage Script Configuration (v3.9+) */}
                  {request.usageScript && (
                    <div className="space-y-3 pt-2 border-t border-border-default">
                      <div className="grid grid-cols-3 items-center gap-4">
                        <div className="font-medium text-sm text-muted-foreground">
                          {t("deeplink.usageScript")}
                        </div>
                        <div className="col-span-2 text-sm">
                          {/* Deeplink scripts are always stored disabled */}
                          <span className="inline-flex items-center px-2 py-0.5 rounded-md text-xs font-medium bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400">
                            {t("deeplink.usageScriptDisabled")}
                          </span>
                        </div>
                      </div>

                      {usageScriptCode !== null && (
                        <UsageScriptPreview
                          code={usageScriptCode}
                          targetHosts={preview?.usageScript?.targetHosts}
                        />
                      )}

                      {/* Usage API Key (if different from provider) */}
                      {request.usageApiKey &&
                        request.usageApiKey !== request.apiKey && (
                          <div className="grid grid-cols-3 items-center gap-4">
                            <div className="font-medium text-sm text-muted-foreground">
                              {t("deeplink.usageApiKey")}
                            </div>
                            <div className="col-span-2 text-sm font-mono text-muted-foreground">
                              {request.usageApiKey.length > 4
                                ? `${request.usageApiKey.substring(0, 4)}${"*".repeat(12)}`
                                : "****"}
                            </div>
                          </div>
                        )}

                      {/* Usage Base URL (if different from provider) */}
                      {request.usageBaseUrl &&
                        request.usageBaseUrl !== request.endpoint && (
                          <div className="grid grid-cols-3 items-center gap-4">
                            <div className="font-medium text-sm text-muted-foreground">
                              {t("deeplink.usageBaseUrl")}
                            </div>
                            <div className="col-span-2 text-sm break-all">
                              {request.usageBaseUrl}
                            </div>
                          </div>
                        )}

                      {/* Auto Query Interval */}
                      {request.usageAutoInterval &&
                        request.usageAutoInterval > 0 && (
                          <div className="grid grid-cols-3 items-center gap-4">
                            <div className="font-medium text-sm text-muted-foreground">
                              {t("deeplink.usageAutoInterval")}
                            </div>
                            <div className="col-span-2 text-sm">
                              {t("deeplink.usageAutoIntervalValue", {
                                minutes: request.usageAutoInterval,
                              })}
                            </div>
                          </div>
                        )}
                    </div>
                  )}

                  {preview?.existing && (
                    <ExistingProviderDiff existing={preview.existing} />
                  )}

                  {preview && <LiveFilesPreview preview={preview} />}

                  {/* Warning */}
                  <div className="rounded-lg bg-yellow-50 dark:bg-yellow-900/20 p-3 text-sm text-yellow-800 dark:text-yellow-200">
                    {t("deeplink.warning")}
                  </div>
                </>
              )}

              {risks.length > 0 && (
                <RiskAcknowledgement
                  checked={riskAcknowledged}
                  onCheckedChange={setRiskAcknowledged}
                />
              )}
            </div>

            <DialogFooter>
              <Button
                variant="outline"
                onClick={handleCancel}
                disabled={isImporting}
              >
                {t("common.cancel")}
              </Button>
              <Button
                onClick={handleImport}
                disabled={isImporting || importBlocked}
              >
                {isImporting ? t("deeplink.importing") : t("deeplink.import")}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
