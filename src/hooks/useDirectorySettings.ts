import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { homeDir, join } from "@tauri-apps/api/path";
import { settingsApi } from "@/lib/api";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { SettingsFormState } from "./useSettingsForm";
import {
  APP_DIRECTORY_META,
  DIRECTORY_APP_IDS,
  sanitizeDir,
  type DirectoryAppId,
} from "./settingsDirectoryFields";

export type { DirectoryAppId };
type DirectoryKey = "appConfig" | DirectoryAppId;

export type ResolvedDirectories = Record<DirectoryKey, string>;

/** Per-field resolution failures from the initial load (message per key). */
export type DirectoryLoadErrors = Partial<Record<DirectoryKey, string>>;

const emptyDirectories = (): ResolvedDirectories => {
  const result = { appConfig: "" } as ResolvedDirectories;
  for (const app of DIRECTORY_APP_IDS) result[app] = "";
  return result;
};

const computeDefaultAppConfigDir = async (): Promise<string | undefined> => {
  try {
    const home = await homeDir();
    return await join(home, ".cc-switch");
  } catch (error) {
    console.error(
      "[useDirectorySettings] Failed to resolve default app config dir",
      error,
    );
    return undefined;
  }
};

const computeDefaultConfigDir = async (
  app: DirectoryAppId,
): Promise<string | undefined> => {
  try {
    const home = await homeDir();
    return await join(home, APP_DIRECTORY_META[app].defaultFolder);
  } catch (error) {
    console.error(
      "[useDirectorySettings] Failed to resolve default config dir",
      error,
    );
    return undefined;
  }
};

export interface UseDirectorySettingsProps {
  settings: SettingsFormState | null;
  onUpdateSettings: (updates: Partial<SettingsFormState>) => void;
}

export interface UseDirectorySettingsResult {
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  isLoading: boolean;
  /** Fields whose current directory could not be resolved on load. */
  loadErrors: DirectoryLoadErrors;
  initialAppConfigDir?: string;
  updateDirectory: (app: DirectoryAppId, value?: string) => void;
  updateAppConfigDir: (value?: string) => void;
  browseDirectory: (app: DirectoryAppId) => Promise<void>;
  browseAppConfigDir: () => Promise<void>;
  resetDirectory: (app: DirectoryAppId) => Promise<void>;
  resetAppConfigDir: () => Promise<void>;
  resetAllDirectories: (overrides?: ResolvedAppDirectoryOverrides) => void;
}

export type ResolvedAppDirectoryOverrides = Partial<
  Record<DirectoryAppId, string | undefined>
>;

/**
 * useDirectorySettings - 目录管理
 * 负责：
 * - appConfigDir 状态
 * - resolvedDirs 状态
 * - 目录选择（browse）
 * - 目录重置
 * - 默认值计算
 */
export function useDirectorySettings({
  settings,
  onUpdateSettings,
}: UseDirectorySettingsProps): UseDirectorySettingsResult {
  const { t } = useTranslation();

  const [appConfigDir, setAppConfigDir] = useState<string | undefined>(
    undefined,
  );
  const [resolvedDirs, setResolvedDirs] =
    useState<ResolvedDirectories>(emptyDirectories);
  const [isLoading, setIsLoading] = useState(true);
  const [loadErrors, setLoadErrors] = useState<DirectoryLoadErrors>({});

  const defaultsRef = useRef<ResolvedDirectories>(emptyDirectories());
  const initialAppConfigDirRef = useRef<string | undefined>(undefined);

  // 加载目录信息
  useEffect(() => {
    let active = true;
    setIsLoading(true);

    const load = async () => {
      // Each field resolves independently (FE-ARCH-45): one app whose
      // directory cannot be resolved (invalid path, unmounted drive) must not
      // blank every other field.
      const settled = <T>(promise: Promise<T>) =>
        promise.then(
          (value) => ({ ok: true as const, value }),
          (error: unknown) => ({ ok: false as const, error }),
        );
      try {
        const [overrideResult, defaultAppConfig, appDirs] = await Promise.all([
          settled(settingsApi.getAppConfigDirOverride()),
          computeDefaultAppConfigDir(),
          Promise.all(
            DIRECTORY_APP_IDS.map((app) =>
              Promise.all([
                settled(settingsApi.getConfigDir(app)),
                computeDefaultConfigDir(app),
              ]),
            ),
          ),
        ]);

        if (!active) return;

        const errors: DirectoryLoadErrors = {};
        if (!overrideResult.ok) {
          console.error(
            "[useDirectorySettings] Failed to load app config dir override",
            overrideResult.error,
          );
          errors.appConfig = extractErrorMessage(overrideResult.error) || "?";
        }
        const normalizedOverride = overrideResult.ok
          ? sanitizeDir(overrideResult.value ?? undefined)
          : undefined;
        const defaults = emptyDirectories();
        defaults.appConfig = defaultAppConfig ?? "";
        const resolved = { ...defaults };
        resolved.appConfig = normalizedOverride ?? defaults.appConfig;
        DIRECTORY_APP_IDS.forEach((app, index) => {
          const [current, fallback] = appDirs[index];
          defaults[app] = fallback ?? "";
          if (current.ok) {
            resolved[app] = current.value || defaults[app];
          } else {
            console.error(
              `[useDirectorySettings] Failed to resolve ${app} config dir`,
              current.error,
            );
            errors[app] = extractErrorMessage(current.error) || "?";
            resolved[app] = defaults[app];
          }
        });
        defaultsRef.current = defaults;

        setAppConfigDir(normalizedOverride);
        initialAppConfigDirRef.current = normalizedOverride;
        setResolvedDirs(resolved);
        setLoadErrors(errors);
      } catch (error) {
        console.error(
          "[useDirectorySettings] Failed to load directory info",
          error,
        );
      } finally {
        if (active) {
          setIsLoading(false);
        }
      }
    };

    void load();
    return () => {
      active = false;
    };
  }, []);

  const updateDirectoryState = useCallback(
    (key: DirectoryKey, value?: string) => {
      const sanitized = sanitizeDir(value);
      // Keep the raw text while the user is typing (trimming per keystroke
      // eats the space in "John Doe"); a blank value clears the override.
      // Trimming happens on blur and when the settings are saved.
      const stored = sanitized === undefined ? undefined : value;
      if (key === "appConfig") {
        setAppConfigDir(stored);
      } else {
        onUpdateSettings({
          [APP_DIRECTORY_META[key].field]: stored,
        });
      }

      setResolvedDirs((prev) => {
        const next = sanitized ?? defaultsRef.current[key];
        // Same-ref early-return: unchanged value shouldn't cascade renders
        // through the settings tree.
        if (prev[key] === next) return prev;
        return { ...prev, [key]: next };
      });
    },
    [onUpdateSettings],
  );

  const updateAppConfigDir = useCallback(
    (value?: string) => {
      updateDirectoryState("appConfig", value);
    },
    [updateDirectoryState],
  );

  const updateDirectory = useCallback(
    (app: DirectoryAppId, value?: string) => {
      updateDirectoryState(app, value);
    },
    [updateDirectoryState],
  );

  const browseDirectory = useCallback(
    async (app: DirectoryAppId) => {
      const currentValue =
        settings?.[APP_DIRECTORY_META[app].field] ?? resolvedDirs[app];

      try {
        const picked = await settingsApi.selectConfigDirectory(currentValue);
        const sanitized = sanitizeDir(picked ?? undefined);
        if (!sanitized) return;
        updateDirectoryState(app, sanitized);
      } catch (error) {
        console.error("[useDirectorySettings] Failed to pick directory", error);
        toast.error(
          t("settings.selectFileFailed", {
            defaultValue: "Please choose a valid SQL backup file",
          }),
        );
      }
    },
    [settings, resolvedDirs, t, updateDirectoryState],
  );

  const browseAppConfigDir = useCallback(async () => {
    const currentValue = appConfigDir ?? resolvedDirs.appConfig;
    try {
      const picked = await settingsApi.selectConfigDirectory(currentValue);
      const sanitized = sanitizeDir(picked ?? undefined);
      if (!sanitized) return;
      updateDirectoryState("appConfig", sanitized);
    } catch (error) {
      console.error(
        "[useDirectorySettings] Failed to pick app config directory",
        error,
      );
      toast.error(
        t("settings.selectFileFailed", {
          defaultValue: "Please choose a valid SQL backup file",
        }),
      );
    }
  }, [appConfigDir, resolvedDirs.appConfig, t, updateDirectoryState]);

  const resetDirectory = useCallback(
    async (app: DirectoryAppId) => {
      if (!defaultsRef.current[app]) {
        const fallback = await computeDefaultConfigDir(app);
        if (fallback) {
          defaultsRef.current = {
            ...defaultsRef.current,
            [app]: fallback,
          };
        }
      }
      updateDirectoryState(app, undefined);
    },
    [updateDirectoryState],
  );

  const resetAppConfigDir = useCallback(async () => {
    if (!defaultsRef.current.appConfig) {
      const fallback = await computeDefaultAppConfigDir();
      if (fallback) {
        defaultsRef.current = {
          ...defaultsRef.current,
          appConfig: fallback,
        };
      }
    }
    updateDirectoryState("appConfig", undefined);
  }, [updateDirectoryState]);

  const resetAllDirectories = useCallback(
    (overrides?: ResolvedAppDirectoryOverrides) => {
      setAppConfigDir(initialAppConfigDirRef.current);
      const next = {
        ...defaultsRef.current,
        appConfig:
          initialAppConfigDirRef.current ?? defaultsRef.current.appConfig,
      };
      for (const app of DIRECTORY_APP_IDS) {
        next[app] = overrides?.[app] ?? defaultsRef.current[app];
      }
      setResolvedDirs(next);
    },
    [],
  );

  return {
    appConfigDir,
    resolvedDirs,
    isLoading,
    loadErrors,
    initialAppConfigDir: initialAppConfigDirRef.current,
    updateDirectory,
    updateAppConfigDir,
    browseDirectory,
    browseAppConfigDir,
    resetDirectory,
    resetAppConfigDir,
    resetAllDirectories,
  };
}
