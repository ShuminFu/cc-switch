import { useCallback, useMemo } from "react";
import { useSettingsQuery } from "@/lib/query";
import { useSettingsForm, type SettingsFormState } from "./useSettingsForm";
import {
  useDirectorySettings,
  type DirectoryAppId,
  type DirectoryLoadErrors,
  type ResolvedDirectories,
} from "./useDirectorySettings";
import { useSettingsMetadata } from "./useSettingsMetadata";
import { directoryValuesByApp, sanitizeDir } from "./settingsDirectoryFields";
import { useSaveSettings, type SaveResult } from "./useSaveSettings";

export interface UseSettingsResult {
  settings: SettingsFormState | null;
  isLoading: boolean;
  isSaving: boolean;
  isPortable: boolean;
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  /** Directory fields that failed to resolve on load (FE-ARCH-45). */
  directoryLoadErrors: DirectoryLoadErrors;
  requiresRestart: boolean;
  updateSettings: (updates: Partial<SettingsFormState>) => void;
  updateDirectory: (app: DirectoryAppId, value?: string) => void;
  updateAppConfigDir: (value?: string) => void;
  browseDirectory: (app: DirectoryAppId) => Promise<void>;
  browseAppConfigDir: () => Promise<void>;
  resetDirectory: (app: DirectoryAppId) => Promise<void>;
  resetAppConfigDir: () => Promise<void>;
  saveSettings: (
    overrides?: Partial<SettingsFormState>,
    options?: { silent?: boolean },
  ) => Promise<SaveResult | null>;
  autoSaveSettings: (
    updates: Partial<SettingsFormState>,
  ) => Promise<SaveResult | null>;
  resetSettings: () => void;
  acknowledgeRestart: () => void;
}

export type { SettingsFormState, ResolvedDirectories };

/**
 * useSettings - 组合层
 * 组合 useSettingsForm、useDirectorySettings、useSettingsMetadata，
 * 并通过 useSaveSettings 这一条保存管线提供即时保存与显式保存。
 */
export function useSettings(): UseSettingsResult {
  const { data } = useSettingsQuery();
  const { save, isSaving } = useSaveSettings();

  const {
    settings,
    isLoading: isFormLoading,
    initialLanguage,
    updateSettings,
    resetSettings: resetForm,
    syncLanguage,
  } = useSettingsForm();

  const {
    appConfigDir,
    resolvedDirs,
    loadErrors: directoryLoadErrors,
    isLoading: isDirectoryLoading,
    initialAppConfigDir,
    updateDirectory,
    updateAppConfigDir,
    browseDirectory,
    browseAppConfigDir,
    resetDirectory,
    resetAppConfigDir,
    resetAllDirectories,
  } = useDirectorySettings({
    settings,
    onUpdateSettings: updateSettings,
  });

  const {
    isPortable,
    requiresRestart,
    isLoading: isMetadataLoading,
    acknowledgeRestart,
    setRequiresRestart,
  } = useSettingsMetadata();

  const resetSettings = useCallback(() => {
    resetForm(data ?? null);
    syncLanguage(initialLanguage);
    resetAllDirectories(directoryValuesByApp(data, sanitizeDir));
    setRequiresRestart(false);
  }, [
    data,
    initialLanguage,
    resetForm,
    syncLanguage,
    resetAllDirectories,
    setRequiresRestart,
  ]);

  // 即时保存（General 等标签页）：只提交 updates 涉及的副作用，目录覆盖沿用已持久化值
  const autoSaveSettings = useCallback(
    (updates: Partial<SettingsFormState>) =>
      save({ scope: "auto", form: settings, updates, data }),
    [data, save, settings],
  );

  // 显式保存（Advanced 标签页的保存按钮）：提交目录覆盖、应用配置目录并判断是否需要重启
  const saveSettings = useCallback(
    async (
      overrides?: Partial<SettingsFormState>,
      options?: { silent?: boolean },
    ) => {
      const result = await save({
        scope: "full",
        form: settings,
        updates: overrides,
        data,
        appConfigDir,
        initialAppConfigDir,
        silent: options?.silent,
      });
      if (result) setRequiresRestart(result.requiresRestart);
      return result;
    },
    [
      appConfigDir,
      data,
      initialAppConfigDir,
      save,
      settings,
      setRequiresRestart,
    ],
  );

  const isLoading = useMemo(
    () => isFormLoading || isDirectoryLoading || isMetadataLoading,
    [isFormLoading, isDirectoryLoading, isMetadataLoading],
  );

  return {
    settings,
    isLoading,
    isSaving,
    isPortable,
    appConfigDir,
    resolvedDirs,
    directoryLoadErrors,
    requiresRestart,
    updateSettings,
    updateDirectory,
    updateAppConfigDir,
    browseDirectory,
    browseAppConfigDir,
    resetDirectory,
    resetAppConfigDir,
    saveSettings,
    autoSaveSettings,
    resetSettings,
    acknowledgeRestart,
  };
}
