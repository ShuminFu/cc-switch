import { useId, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { DeepLinkImportRequest } from "../../lib/api/deeplink";
import { decodeBase64Utf8 } from "../../lib/utils/base64";
import { Checkbox } from "@/components/ui/checkbox";

/**
 * Prompt import preview. The whole content is shown (scrollable, with a line
 * count) because a prompt becomes part of the coding tool's system prompt;
 * anything hidden past a preview cut-off could be a prompt injection
 * (SESS-07). Enabling — which replaces the live CLAUDE.md / AGENTS.md — is a
 * separate explicit opt-in that starts unticked.
 */
export function PromptConfirmation({
  request,
  enableNow = false,
  onEnableNowChange,
}: {
  request: DeepLinkImportRequest;
  enableNow?: boolean;
  onEnableNowChange?: (value: boolean) => void;
}) {
  const { t } = useTranslation();
  const enableId = useId();

  const decodedContent = useMemo(() => {
    if (!request.content) return "";
    return decodeBase64Utf8(request.content);
  }, [request.content]);
  const lineCount = decodedContent ? decodedContent.split("\n").length : 0;

  return (
    <div className="space-y-4">
      <h3 className="text-lg font-semibold">{t("deeplink.prompt.title")}</h3>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          {t("deeplink.prompt.app")}
        </label>
        <div className="mt-1 text-sm capitalize">{request.app}</div>
      </div>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          {t("deeplink.prompt.name")}
        </label>
        <div className="mt-1 text-sm">{request.name}</div>
      </div>

      {request.description && (
        <div>
          <label className="block text-sm font-medium text-muted-foreground">
            {t("deeplink.prompt.description")}
          </label>
          <div className="mt-1 text-sm">{request.description}</div>
        </div>
      )}

      <div>
        <label className="flex items-baseline justify-between text-sm font-medium text-muted-foreground">
          <span>{t("deeplink.prompt.contentPreview")}</span>
          <span className="text-xs font-normal">
            {t("deeplink.prompt.lineCount", {
              defaultValue: "{{count}} lines, {{chars}} characters",
              count: lineCount,
              chars: decodedContent.length,
            })}
          </span>
        </label>
        <pre
          className="mt-1 max-h-72 overflow-auto bg-muted/50 p-2 rounded text-xs whitespace-pre-wrap break-words border"
          data-testid="deeplink-prompt-content"
        >
          {decodedContent}
        </pre>
      </div>

      {onEnableNowChange && (
        <div className="space-y-1 rounded-lg border p-3">
          {request.enabled && (
            <div className="text-xs text-yellow-700 dark:text-yellow-400">
              {t("deeplink.prompt.enabledWarning")}
            </div>
          )}
          <div className="flex items-start gap-2">
            <Checkbox
              id={enableId}
              checked={enableNow}
              onCheckedChange={(v) => onEnableNowChange(v === true)}
            />
            <label htmlFor={enableId} className="text-sm leading-tight">
              {t("deeplink.prompt.enableNowLabel", {
                defaultValue:
                  "Enable now: replace the live {{app}} prompt file with this content (the current file is saved as a backup prompt)",
                app: request.app,
              })}
            </label>
          </div>
        </div>
      )}
    </div>
  );
}
