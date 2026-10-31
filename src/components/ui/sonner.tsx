import { Toaster as SonnerToaster, toast, type ExternalToast } from "sonner";
import i18n from "i18next";
import { useTheme } from "@/components/theme-provider";
import { copyText } from "@/lib/clipboard";
import { redactSecrets } from "@/utils/redact";

/** Error toasts often carry a description; give users time to read it. */
export const ERROR_TOAST_DURATION_MS = 6000;
/** At most this many toasts are stacked at once (FE-ARCH-36). */
export const MAX_VISIBLE_TOASTS = 4;

const redactNode = <T,>(value: T): T =>
  (typeof value === "string" ? redactSecrets(value) : value) as T;

/**
 * Defaults for every `toast.error` (FE-UI-13, FE-ARCH-36, FE-PROV-23):
 * - stays 6 s and has a close button;
 * - secrets in the title/description are masked before display;
 * - a "Copy" action puts the (redacted) text on the clipboard unless the
 *   caller supplied its own action.
 * An explicit option from the caller always wins.
 */
export function errorToastOptions(
  message: unknown,
  data?: ExternalToast,
): ExternalToast {
  const description = redactNode(data?.description);
  const parts = [message, description].filter(
    (part): part is string => typeof part === "string" && part.length > 0,
  );
  const options: ExternalToast = {
    duration: ERROR_TOAST_DURATION_MS,
    closeButton: true,
    ...data,
    description,
  };
  if (!data?.action && parts.length > 0) {
    options.action = {
      label: i18n.t("common.copy", { defaultValue: "Copy" }),
      onClick: () => {
        void copyText(parts.join("\n")).catch(() => undefined);
      },
    };
  }
  return options;
}

const baseError = toast.error;
if (!(baseError as { __ccSwitchPatched?: boolean }).__ccSwitchPatched) {
  const patched: typeof toast.error = (message, data) => {
    const safeMessage = redactNode(message);
    return baseError(safeMessage, errorToastOptions(safeMessage, data));
  };
  (patched as { __ccSwitchPatched?: boolean }).__ccSwitchPatched = true;
  toast.error = patched;
}

export function Toaster() {
  const { theme } = useTheme();

  // 将应用主题映射到 Sonner 的主题
  // 如果是 "system"，Sonner 会自己处理
  const sonnerTheme = theme === "system" ? "system" : theme;

  return (
    <SonnerToaster
      position="top-center"
      richColors
      theme={sonnerTheme}
      visibleToasts={MAX_VISIBLE_TOASTS}
      toastOptions={{
        duration: 2000,
        classNames: {
          toast:
            "group rounded-md border bg-background text-foreground shadow-lg",
          title: "text-sm font-semibold",
          description: "text-sm text-muted-foreground",
          closeButton:
            "absolute right-2 top-2 rounded-full p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground",
          actionButton:
            "rounded-md bg-primary px-3 py-1 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90",
        },
      }}
    />
  );
}
