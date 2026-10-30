import { Toaster as SonnerToaster, toast } from "sonner";
import { useTheme } from "@/components/theme-provider";

/** Error toasts often carry a description; give users time to read it. */
export const ERROR_TOAST_DURATION_MS = 6000;

// Raise the default duration of every `toast.error` call app-wide (FE-UI-13)
// without touching each call site; an explicit `duration` still wins.
const baseError = toast.error;
if (!(baseError as { __ccSwitchPatched?: boolean }).__ccSwitchPatched) {
  const patched: typeof toast.error = (message, data) =>
    baseError(message, { duration: ERROR_TOAST_DURATION_MS, ...data });
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
