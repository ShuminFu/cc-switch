import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsQuery } from "@/lib/query";
import type { Settings } from "@/types";
import {
  DIRECTORY_SETTINGS_FIELDS,
  sanitizeDir,
  sanitizeDirectoryFields,
} from "./settingsDirectoryFields";

type Language = "zh" | "zh-TW" | "en" | "ja";

export type SettingsFormState = Omit<Settings, "language"> & {
  language: Language;
};

const normalizeLanguage = (lang?: string | null): Language => {
  if (!lang) return "zh";
  const normalized = lang.toLowerCase().replace(/_/g, "-");

  if (normalized === "zh") {
    return "zh";
  }

  if (
    normalized === "zh-tw" ||
    normalized.startsWith("zh-hant") ||
    normalized.startsWith("zh-hk") ||
    normalized.startsWith("zh-mo")
  ) {
    return "zh-TW";
  }

  if (normalized === "en" || normalized === "ja") {
    return normalized;
  }

  if (normalized.startsWith("zh")) {
    return "zh";
  }

  return "zh";
};

const isSupportedLanguage = (lang?: string | null): boolean => {
  if (!lang) return false;
  const normalized = lang.toLowerCase().replace(/_/g, "-");
  return (
    normalized === "en" || normalized === "ja" || normalized.startsWith("zh")
  );
};

export interface UseSettingsFormResult {
  settings: SettingsFormState | null;
  isLoading: boolean;
  initialLanguage: Language;
  updateSettings: (updates: Partial<SettingsFormState>) => void;
  resetSettings: (serverData: Settings | null) => void;
  readPersistedLanguage: () => Language;
  syncLanguage: (lang: Language) => void;
}

/**
 * useSettingsForm - 表单状态管理
 * 负责：
 * - 表单数据状态
 * - 表单字段更新
 * - 语言同步
 * - 表单重置
 */
export function useSettingsForm(): UseSettingsFormResult {
  const { i18n } = useTranslation();
  const { data, isLoading } = useSettingsQuery();

  const [settingsState, setSettingsState] = useState<SettingsFormState | null>(
    null,
  );

  const initialLanguageRef = useRef<Language>("zh");
  const lastServerDataRef = useRef<Settings | null>(null);

  const readPersistedLanguage = useCallback((): Language => {
    if (typeof window !== "undefined") {
      const stored = window.localStorage.getItem("language");
      if (isSupportedLanguage(stored)) {
        return normalizeLanguage(stored);
      }
    }
    return normalizeLanguage(i18n.language);
  }, [i18n]);

  const syncLanguage = useCallback(
    (lang: Language) => {
      const current = normalizeLanguage(i18n.language);
      if (current !== lang) {
        void i18n.changeLanguage(lang);
      }
    },
    [i18n],
  );

  // 初始化设置数据
  useEffect(() => {
    if (!data) return;

    const normalizedLanguage = normalizeLanguage(
      data.language ?? readPersistedLanguage(),
    );

    const normalized: SettingsFormState = {
      ...data,
      showInTray: data.showInTray ?? true,
      minimizeToTrayOnClose: data.minimizeToTrayOnClose ?? true,
      useAppWindowControls: data.useAppWindowControls ?? false,
      enableClaudePluginIntegration:
        data.enableClaudePluginIntegration ?? false,
      silentStartup: data.silentStartup ?? false,
      skipClaudeOnboarding: data.skipClaudeOnboarding ?? false,
      preserveCodexOfficialAuthOnSwitch:
        data.preserveCodexOfficialAuthOnSwitch ?? false,
      unifyCodexSessionHistory: data.unifyCodexSessionHistory ?? false,
      ...sanitizeDirectoryFields(data),
      language: normalizedLanguage,
    };

    // 目录覆盖是 Advanced 标签页上需要显式保存的草稿：其它设置自动保存后
    // 会触发 settings 重新拉取，此时保留用户尚未保存的目录编辑，
    // 否则 Save 时就丢失了这些修改（以及对应的 live 同步）。
    const previousServer = lastServerDataRef.current;
    lastServerDataRef.current = data;
    setSettingsState((prev) => {
      if (!prev || !previousServer) return normalized;
      const next = { ...normalized };
      for (const field of DIRECTORY_SETTINGS_FIELDS) {
        const draft = prev[field];
        if (sanitizeDir(draft) !== sanitizeDir(previousServer[field])) {
          next[field] = draft;
        }
      }
      return next;
    });
    initialLanguageRef.current = normalizedLanguage;
    syncLanguage(normalizedLanguage);
  }, [data, readPersistedLanguage, syncLanguage]);

  const updateSettings = useCallback(
    (updates: Partial<SettingsFormState>) => {
      setSettingsState((prev) => {
        const base =
          prev ??
          ({
            showInTray: true,
            minimizeToTrayOnClose: true,
            useAppWindowControls: false,
            enableClaudePluginIntegration: false,
            skipClaudeOnboarding: false,
            preserveCodexOfficialAuthOnSwitch: false,
            unifyCodexSessionHistory: false,
            language: readPersistedLanguage(),
          } as SettingsFormState);

        const next: SettingsFormState = {
          ...base,
          ...updates,
        };

        if (updates.language) {
          const normalized = normalizeLanguage(updates.language);
          next.language = normalized;
          syncLanguage(normalized);
        }

        return next;
      });
    },
    [readPersistedLanguage, syncLanguage],
  );

  const resetSettings = useCallback(
    (serverData: Settings | null) => {
      if (!serverData) return;

      const normalizedLanguage = normalizeLanguage(
        serverData.language ?? readPersistedLanguage(),
      );

      const normalized: SettingsFormState = {
        ...serverData,
        showInTray: serverData.showInTray ?? true,
        minimizeToTrayOnClose: serverData.minimizeToTrayOnClose ?? true,
        useAppWindowControls: serverData.useAppWindowControls ?? false,
        enableClaudePluginIntegration:
          serverData.enableClaudePluginIntegration ?? false,
        silentStartup: serverData.silentStartup ?? false,
        skipClaudeOnboarding: serverData.skipClaudeOnboarding ?? false,
        preserveCodexOfficialAuthOnSwitch:
          serverData.preserveCodexOfficialAuthOnSwitch ?? false,
        unifyCodexSessionHistory: serverData.unifyCodexSessionHistory ?? false,
        ...sanitizeDirectoryFields(serverData),
        language: normalizedLanguage,
      };

      lastServerDataRef.current = serverData;
      setSettingsState(normalized);
      syncLanguage(initialLanguageRef.current);
    },
    [readPersistedLanguage, syncLanguage],
  );

  return {
    settings: settingsState,
    isLoading,
    initialLanguage: initialLanguageRef.current,
    updateSettings,
    resetSettings,
    readPersistedLanguage,
    syncLanguage,
  };
}
