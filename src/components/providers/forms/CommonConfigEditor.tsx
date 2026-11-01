import { useDarkMode } from "@/hooks/useDarkMode";
import { useTranslation } from "react-i18next";
import { useEffect, useState, useCallback, useMemo } from "react";
import { Label } from "@/components/ui/label";
import JsonEditor from "@/components/JsonEditor";
import { CommonConfigModal } from "./CommonConfigModal";

interface CommonConfigEditorProps {
  value: string;
  onChange: (value: string) => void;
  useCommonConfig: boolean;
  onCommonConfigToggle: (checked: boolean) => void;
  commonConfigSnippet: string;
  onCommonConfigSnippetChange: (value: string) => void;
  commonConfigError: string;
  onEditClick: () => void;
  isModalOpen: boolean;
  onModalClose: () => void;
  onExtract?: () => void;
  isExtracting?: boolean;
}

export function CommonConfigEditor({
  value,
  onChange,
  useCommonConfig,
  onCommonConfigToggle,
  commonConfigSnippet,
  onCommonConfigSnippetChange,
  commonConfigError,
  onEditClick,
  isModalOpen,
  onModalClose,
  onExtract,
  isExtracting,
}: CommonConfigEditorProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();

  // Mirror value prop to local state so checkbox toggles and JsonEditor stay in sync
  // (parent uses form.getValues which doesn't trigger re-renders)
  const [localValue, setLocalValue] = useState(value);

  useEffect(() => {
    setLocalValue(value);
  }, [value]);

  const handleLocalChange = useCallback(
    (newValue: string) => {
      setLocalValue(newValue);
      onChange(newValue);
    },
    [onChange],
  );

  const toggleStates = useMemo(() => {
    try {
      const config = JSON.parse(localValue);
      return {
        hideAttribution:
          config?.attribution?.commit === "" && config?.attribution?.pr === "",
        teammates:
          config?.env?.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS === "1" ||
          config?.env?.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS === 1,
        enableToolSearch:
          config?.env?.ENABLE_TOOL_SEARCH === "true" ||
          config?.env?.ENABLE_TOOL_SEARCH === "1",
        effortMax: config?.env?.CLAUDE_CODE_EFFORT_LEVEL === "max",
        disableAutoUpgrade:
          config?.env?.DISABLE_AUTOUPDATER === "1" ||
          config?.env?.DISABLE_AUTOUPDATER === 1,
      };
    } catch {
      return {
        hideAttribution: false,
        teammates: false,
        enableToolSearch: false,
        effortMax: false,
        disableAutoUpgrade: false,
      };
    }
  }, [localValue]);

  const handleToggle = useCallback(
    (toggleKey: string, checked: boolean) => {
      try {
        const config = JSON.parse(localValue || "{}");

        switch (toggleKey) {
          case "hideAttribution":
            if (checked) {
              config.attribution = { commit: "", pr: "" };
            } else {
              delete config.attribution;
            }
            break;
          case "teammates":
            if (!config.env) config.env = {};
            if (checked) {
              config.env.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = "1";
            } else {
              delete config.env.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS;
              if (Object.keys(config.env).length === 0) delete config.env;
            }
            break;
          case "enableToolSearch":
            if (!config.env) config.env = {};
            if (checked) {
              config.env.ENABLE_TOOL_SEARCH = "true";
            } else {
              delete config.env.ENABLE_TOOL_SEARCH;
              if (Object.keys(config.env).length === 0) delete config.env;
            }
            break;
          case "effortMax":
            if (!config.env) config.env = {};
            if (checked) {
              config.env.CLAUDE_CODE_EFFORT_LEVEL = "max";
            } else {
              delete config.env.CLAUDE_CODE_EFFORT_LEVEL;
              if (Object.keys(config.env).length === 0) delete config.env;
            }
            break;
          case "disableAutoUpgrade":
            if (!config.env) config.env = {};
            if (checked) {
              config.env.DISABLE_AUTOUPDATER = "1";
            } else {
              delete config.env.DISABLE_AUTOUPDATER;
              if (Object.keys(config.env).length === 0) delete config.env;
            }
            break;
        }

        handleLocalChange(JSON.stringify(config, null, 2));
      } catch {
        // Don't modify if JSON is invalid
      }
    },
    [localValue, handleLocalChange],
  );

  return (
    <>
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
          <div className="flex items-center gap-2">
            <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
              <input
                type="checkbox"
                id="useCommonConfig"
                checked={useCommonConfig}
                onChange={(e) => onCommonConfigToggle(e.target.checked)}
                className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
              />
              <span>
                {t("claudeConfig.writeCommonConfig", {
                  defaultValue: "Apply Common Config",
                })}
              </span>
            </label>
          </div>
        </div>
        <div className="flex items-center justify-end">
          <button
            type="button"
            onClick={onEditClick}
            className="text-xs text-blue-400 dark:text-blue-500 hover:text-blue-500 dark:hover:text-blue-400 transition-colors"
          >
            {t("claudeConfig.editCommonConfig", {
              defaultValue: "Edit Common Config",
            })}
          </button>
        </div>
        {commonConfigError && !isModalOpen && (
          <p className="text-xs text-red-500 dark:text-red-400 text-right">
            {commonConfigError}
          </p>
        )}
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
          <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
            <input
              type="checkbox"
              checked={toggleStates.hideAttribution}
              onChange={(e) =>
                handleToggle("hideAttribution", e.target.checked)
              }
              className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
            />
            <span>{t("claudeConfig.hideAttribution")}</span>
          </label>
          <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
            <input
              type="checkbox"
              checked={toggleStates.teammates}
              onChange={(e) => handleToggle("teammates", e.target.checked)}
              className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
            />
            <span>{t("claudeConfig.enableTeammates")}</span>
          </label>
          <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
            <input
              type="checkbox"
              checked={toggleStates.enableToolSearch}
              onChange={(e) =>
                handleToggle("enableToolSearch", e.target.checked)
              }
              className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
            />
            <span>{t("claudeConfig.enableToolSearch")}</span>
          </label>
          <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
            <input
              type="checkbox"
              checked={toggleStates.effortMax}
              onChange={(e) => handleToggle("effortMax", e.target.checked)}
              className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
            />
            <span>{t("claudeConfig.effortMax")}</span>
          </label>
          <label className="inline-flex items-center gap-2 text-sm text-muted-foreground cursor-pointer">
            <input
              type="checkbox"
              checked={toggleStates.disableAutoUpgrade}
              onChange={(e) =>
                handleToggle("disableAutoUpgrade", e.target.checked)
              }
              className="w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2"
            />
            <span>{t("claudeConfig.disableAutoUpgrade")}</span>
          </label>
        </div>
        <JsonEditor
          value={localValue}
          onChange={handleLocalChange}
          placeholder={`{
  "env": {
    "ANTHROPIC_BASE_URL": "https://your-api-endpoint.com",
    "ANTHROPIC_AUTH_TOKEN": "your-api-key-here"
  }
}`}
          darkMode={isDarkMode}
          rows={14}
          showValidation={true}
          language="json"
        />
      </div>

      <CommonConfigModal
        isOpen={isModalOpen}
        onClose={onModalClose}
        title={t("claudeConfig.editCommonConfigTitle", {
          defaultValue: "Edit Common Config Snippet",
        })}
        value={commonConfigSnippet}
        onChange={onCommonConfigSnippetChange}
        language="json"
        placeholder={`{
  "env": {
    "ANTHROPIC_BASE_URL": "https://your-api-endpoint.com"
  }
}`}
        error={commonConfigError}
        onExtract={onExtract}
        isExtracting={isExtracting}
        extractLabel={t("claudeConfig.extractFromCurrent", {
          defaultValue: "Extract from Editor",
        })}
      />
    </>
  );
}
