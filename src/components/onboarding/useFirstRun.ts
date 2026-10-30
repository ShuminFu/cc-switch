import { useCallback, useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useSettingsQuery } from "@/lib/query";
import { settingsApi } from "@/lib/api";
import type { Settings, VisibleApps } from "@/types";
import { queryKeys } from "@/lib/query/keys";

/** Window event used to re-open the setup wizard from anywhere (e.g. Settings → About). */
export const OPEN_FIRST_RUN_EVENT = "cc-switch:open-first-run-wizard";

/** Ask the mounted `<FirstRunWizard />` to open again. */
export function openFirstRunWizard(): void {
  globalThis.dispatchEvent(new Event(OPEN_FIRST_RUN_EVENT));
}

/**
 * Whether the first-run wizard should be shown automatically.
 * The backend leaves `firstRunNoticeConfirmed` unset only on a fresh install;
 * every other startup writes `true` before the UI reads settings.
 */
export function shouldAutoOpenFirstRun(
  settings: Settings | undefined,
): boolean {
  return settings != null && settings.firstRunNoticeConfirmed !== true;
}

export interface CompleteFirstRunOptions {
  visibleApps?: VisibleApps;
}

export interface UseFirstRunResult {
  /** Settings are loaded and the wizard is visible. */
  isOpen: boolean;
  /** True when the wizard was opened manually (not the fresh-install case). */
  reopened: boolean;
  settings: Settings | undefined;
  open: () => void;
  /** Persist `firstRunNoticeConfirmed` (plus optional choices) and close. */
  complete: (options?: CompleteFirstRunOptions) => Promise<void>;
}

export function useFirstRun(): UseFirstRunResult {
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();
  const [manualOpen, setManualOpen] = useState(false);
  // Closed locally right after completion, so the dialog does not flash back
  // while the settings query refetches.
  const [dismissed, setDismissed] = useState(false);

  const open = useCallback(() => {
    setDismissed(false);
    setManualOpen(true);
  }, []);

  useEffect(() => {
    const handler = () => open();
    globalThis.addEventListener(OPEN_FIRST_RUN_EVENT, handler);
    return () => globalThis.removeEventListener(OPEN_FIRST_RUN_EVENT, handler);
  }, [open]);

  const autoOpen = shouldAutoOpenFirstRun(settings);
  const isOpen = settings != null && !dismissed && (manualOpen || autoOpen);

  const complete = useCallback(
    async (options: CompleteFirstRunOptions = {}) => {
      setDismissed(true);
      setManualOpen(false);
      try {
        const latest = (await settingsApi.get()) ?? settings;
        if (!latest) return;
        const { webdavSync: _, ...rest } = latest;
        await settingsApi.save({
          ...rest,
          ...(options.visibleApps ? { visibleApps: options.visibleApps } : {}),
          firstRunNoticeConfirmed: true,
        });
        await queryClient.invalidateQueries({
          queryKey: queryKeys.settings.all,
        });
      } catch (error) {
        console.error("[useFirstRun] Failed to save first-run state:", error);
      }
    },
    [queryClient, settings],
  );

  return {
    isOpen,
    reopened: manualOpen && !autoOpen,
    settings,
    open,
    complete,
  };
}
