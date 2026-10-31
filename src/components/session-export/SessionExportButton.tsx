import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Download } from "lucide-react";
import type { SessionMessage, SessionMeta } from "@/types";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  formatSession,
  sessionExportFileName,
  type SessionExportFormat,
} from "./sessionExport";

/**
 * Ask the backend to show a save dialog and write `content` to the chosen
 * file. The path never comes from the webview. Resolves to the saved path,
 * or null when the user cancelled.
 */
export const saveSessionExport = (
  defaultName: string,
  format: SessionExportFormat,
  content: string,
): Promise<string | null> =>
  invoke("save_session_export", { defaultName, format, content });

/** "Export" menu (Markdown / JSON) for the open session (PRODUCT-48). */
export function SessionExportButton({
  session,
  messages,
  disabled,
}: {
  session: SessionMeta;
  messages: SessionMessage[];
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);

  const run = async (format: SessionExportFormat) => {
    setBusy(true);
    try {
      const saved = await saveSessionExport(
        sessionExportFileName(session, format),
        format,
        formatSession(session, messages, format),
      );
      if (saved) {
        toast.success(
          t("sessionExport.saved", {
            defaultValue: "Session exported to {{path}}",
            path: saved,
          }),
        );
      }
    } catch (error) {
      toast.error(
        t("sessionExport.failed", {
          defaultValue: "Export failed: {{error}}",
          error: extractErrorMessage(error) || String(error),
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
          size="sm"
          variant="outline"
          className="gap-1.5"
          disabled={disabled || busy || messages.length === 0}
          aria-label={t("sessionExport.export", { defaultValue: "Export" })}
        >
          <Download className="size-3.5" />
          <span className="hidden sm:inline">
            {t("sessionExport.export", { defaultValue: "Export" })}
          </span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onSelect={() => void run("markdown")}>
          {t("sessionExport.markdown", { defaultValue: "Markdown (.md)" })}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => void run("json")}>
          {t("sessionExport.json", { defaultValue: "JSON (.json)" })}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
