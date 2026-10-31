/**
 * One way to show an error toast (DOCS-19): a localized title, the error's
 * detail (structured backend errors rendered in the UI language), and a
 * "Copy details" action whose text is ready to paste into a bug report.
 *
 * Use it instead of `toast.error(String(error))` / `toast.error(e.message)`,
 * which show raw, often Chinese, backend text with nothing to copy.
 */
import i18n from "i18next";
import { toast } from "sonner";
import { getErrorCode } from "@/lib/errors/appError";
import { extractErrorMessage } from "@/utils/errorUtils";

export interface ShowErrorOptions {
  /** Localized, user-facing summary ("Failed to save settings"). */
  title: string;
  /** The caught value. */
  error?: unknown;
  /** A next step instead of "Copy details" (e.g. open the settings tab). */
  action?: { label: string; onClick: () => void };
}

/** The error's detail in the UI language, or "" when it has none. */
export function errorDetail(error: unknown): string {
  return extractErrorMessage(error).trim();
}

/** Plain text for "Copy details": title, code and detail. */
export function errorReport(title: string, error: unknown): string {
  const lines = [title];
  const code = getErrorCode(error);
  if (code) lines.push(`Code: ${code}`);
  const detail = errorDetail(error);
  if (detail && detail !== title) lines.push(`Detail: ${detail}`);
  return lines.join("\n");
}

async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    toast.success(
      i18n.t("errors.detailsCopied", { defaultValue: "Details copied" }),
    );
  } catch (copyError) {
    console.error("[showError] clipboard write failed", copyError);
  }
}

export function showError({ title, error, action }: ShowErrorOptions): void {
  const detail = errorDetail(error);
  toast.error(title, {
    description: detail && detail !== title ? detail : undefined,
    action: action ?? {
      label: i18n.t("errors.copyDetails", { defaultValue: "Copy details" }),
      onClick: () => void copyText(errorReport(title, error)),
    },
  });
}
