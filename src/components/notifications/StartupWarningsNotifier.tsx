import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { toast } from "sonner";
import { settingsApi, type StartupWarning } from "@/lib/api/settings";
import { useTauriEvent } from "@/hooks/useTauriEvent";

const DISMISSED_KEY = "startup_warnings_dismissed";

/** Stable identity of a warning, used for de-duplication and dismissal. */
const startupWarningId = (w: StartupWarning) =>
  [w.kind, w.appType ?? "", w.path ?? ""].join("|");

const readDismissed = (): Set<string> => {
  try {
    const raw = sessionStorage.getItem(DISMISSED_KEY);
    return new Set(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
};

const rememberDismissed = (id: string) => {
  try {
    const next = readDismissed();
    next.add(id);
    sessionStorage.setItem(DISMISSED_KEY, JSON.stringify([...next]));
  } catch {
    // storage unavailable: the warning may show again after a reload
  }
};

/** Localized title and description for one startup warning kind. */
function describeStartupWarning(
  t: TFunction,
  w: StartupWarning,
): { title: string; description: string } {
  switch (w.kind) {
    case "settings_corrupt":
      return {
        title: t("startupWarnings.settingsCorrupt.title", {
          defaultValue: "Settings file could not be read",
        }),
        description: t("startupWarnings.settingsCorrupt.description", {
          defaultValue:
            "settings.json was invalid and has been moved aside to {{path}}. Default settings are in use; review them in Settings.",
          path: w.path ?? "",
        }),
      };
    case "config_dir_unavailable":
      return {
        title: t("startupWarnings.configDirUnavailable.title", {
          defaultValue: "Data directory unavailable",
        }),
        description: t("startupWarnings.configDirUnavailable.description", {
          defaultValue:
            "The configured data directory {{path}} does not exist. CC Switch is using the default directory; check Settings > Advanced > Directories.",
          path: w.path ?? "",
        }),
      };
    case "proxy_restore_failed":
      return {
        title: t("startupWarnings.proxyRestoreFailed.title", {
          defaultValue: "Proxy takeover could not be restored",
        }),
        description: t("startupWarnings.proxyRestoreFailed.description", {
          defaultValue:
            "Restoring the proxy takeover for {{app}} failed: {{error}}. Open Settings > Proxy to re-enable it.",
          app: w.appType ?? "",
          error: w.message,
        }),
      };
    default:
      return {
        title: t("startupWarnings.generic.title", {
          defaultValue: "Startup warning",
        }),
        description: w.message,
      };
  }
}

/**
 * Shows non-fatal startup problems (`get_startup_warnings`) and later
 * `proxy-restore-failed` events as dismissible toasts. Each warning is shown
 * once per session; closing it keeps it hidden after a reload.
 */
export function StartupWarningsNotifier() {
  const { t } = useTranslation();
  const shown = useRef<Set<string>>(new Set());

  const show = useCallback(
    (w: StartupWarning) => {
      const id = startupWarningId(w);
      if (shown.current.has(id) || readDismissed().has(id)) return;
      shown.current.add(id);
      const { title, description } = describeStartupWarning(t, w);
      toast.warning(title, {
        id: `startup-warning:${id}`,
        description,
        duration: Infinity,
        closeButton: true,
        onDismiss: () => rememberDismissed(id),
      });
    },
    [t],
  );

  useEffect(() => {
    let cancelled = false;
    settingsApi
      .getStartupWarnings()
      .then((warnings) => {
        if (!cancelled) warnings.forEach(show);
      })
      .catch((error) => {
        console.error("[StartupWarnings] failed to load warnings", error);
      });
    return () => {
      cancelled = true;
    };
  }, [show]);

  useTauriEvent("proxy-restore-failed", (payload) => {
    show({
      kind: "proxy_restore_failed",
      appType: payload.appType,
      message: payload.error,
    });
  });

  return null;
}
