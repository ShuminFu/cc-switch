import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsQuery } from "@/lib/query";
import type { Settings } from "@/types";
import {
  DIRECTORY_SETTINGS_FIELDS,
  sanitizeDir,
} from "./settingsDirectoryFields";
import {
  SETTINGS_FORM_DEFAULTS,
  isSupportedLanguage,
  normalizeLanguage,
  normalizeSettingsForm,
  type Language,
  type SettingsFormState,
} from "./settingsPayload";

export type { SettingsFormState };

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
  // State (for render) plus a ref (for callbacks that must see the latest
  // value without being recreated).
  const [initialLanguage, setInitialLanguage] = useState<Language>("zh");
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

    const normalized = normalizeSettingsForm(data, readPersistedLanguage);

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
    initialLanguageRef.current = normalized.language;
    setInitialLanguage(normalized.language);
    syncLanguage(normalized.language);
  }, [data, readPersistedLanguage, syncLanguage]);

  const updateSettings = useCallback(
    (updates: Partial<SettingsFormState>) => {
      setSettingsState((prev) => {
        const base =
          prev ??
          ({
            ...SETTINGS_FORM_DEFAULTS,
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
      lastServerDataRef.current = serverData;
      setSettingsState(
        normalizeSettingsForm(serverData, readPersistedLanguage),
      );
      syncLanguage(initialLanguageRef.current);
    },
    [readPersistedLanguage, syncLanguage],
  );

  return {
    settings: settingsState,
    isLoading,
    initialLanguage,
    updateSettings,
    resetSettings,
    readPersistedLanguage,
    syncLanguage,
  };
}
