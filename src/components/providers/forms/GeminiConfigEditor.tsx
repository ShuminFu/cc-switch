import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { GeminiEnvSection, GeminiConfigSection } from "./GeminiConfigSections";
import { CommonConfigModal } from "./CommonConfigModal";

interface GeminiConfigEditorProps {
  envValue: string;
  configValue: string;
  onEnvChange: (value: string) => void;
  onConfigChange: (value: string) => void;
  onEnvBlur?: () => void;
  useCommonConfig: boolean;
  onCommonConfigToggle: (checked: boolean) => void;
  commonConfigSnippet: string;
  onCommonConfigSnippetChange: (value: string) => boolean;
  onCommonConfigErrorClear: () => void;
  commonConfigError: string;
  envError: string;
  configError: string;
  onExtract?: () => void;
  isExtracting?: boolean;
}

const GeminiConfigEditor: React.FC<GeminiConfigEditorProps> = ({
  envValue,
  configValue,
  onEnvChange,
  onConfigChange,
  onEnvBlur,
  useCommonConfig,
  onCommonConfigToggle,
  commonConfigSnippet,
  onCommonConfigSnippetChange,
  onCommonConfigErrorClear,
  commonConfigError,
  envError,
  configError,
  onExtract,
  isExtracting,
}) => {
  const { t } = useTranslation();
  const [isCommonConfigModalOpen, setIsCommonConfigModalOpen] = useState(false);

  const handleCloseCommonConfigModal = () => {
    onCommonConfigErrorClear();
    setIsCommonConfigModalOpen(false);
  };

  return (
    <div className="space-y-6">
      {/* Env Section */}
      <GeminiEnvSection
        value={envValue}
        onChange={onEnvChange}
        onBlur={onEnvBlur}
        error={envError}
        useCommonConfig={useCommonConfig}
        onCommonConfigToggle={onCommonConfigToggle}
        onEditCommonConfig={() => setIsCommonConfigModalOpen(true)}
        commonConfigError={commonConfigError}
      />

      {/* Config JSON Section */}
      <GeminiConfigSection
        value={configValue}
        onChange={onConfigChange}
        configError={configError}
      />

      {/* Common Config Modal */}
      <CommonConfigModal
        isOpen={isCommonConfigModalOpen}
        onClose={handleCloseCommonConfigModal}
        title={t("geminiConfig.editCommonConfigTitle", {
          defaultValue: "Edit Gemini Common Config Snippet",
        })}
        hint={
          <p className="text-xs text-amber-600 dark:text-amber-400">
            {t("geminiConfig.commonConfigHint", {
              defaultValue:
                "This snippet writes to Gemini .env when 'Apply Common Config' is checked (GOOGLE_GEMINI_BASE_URL and GEMINI_API_KEY are not allowed)",
            })}
          </p>
        }
        value={commonConfigSnippet}
        language="json"
        placeholder={`{
  "GEMINI_MODEL": "gemini-3.5-flash"
}`}
        onSave={onCommonConfigSnippetChange}
        error={commonConfigError}
        onExtract={onExtract}
        isExtracting={isExtracting}
        extractLabel={t("geminiConfig.extractFromCurrent", {
          defaultValue: "Extract from Editor",
        })}
      />
    </div>
  );
};

export default GeminiConfigEditor;
