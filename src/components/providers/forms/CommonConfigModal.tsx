import { useEffect, useState, type ReactNode } from "react";
import { Save, Download, Loader2, Package } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useDarkMode } from "@/hooks/useDarkMode";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import JsonEditor from "@/components/JsonEditor";

type EditorLanguage = "json" | "toml" | "dotenv";

interface CommonConfigModalBaseProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  /** Format-specific hint shown under the guide. */
  hint?: ReactNode;
  value: string;
  language: EditorLanguage;
  placeholder: string;
  error?: string;
  onExtract?: () => void;
  isExtracting?: boolean;
  extractLabel?: string;
}

/**
 * Draft mode: edits stay local until Save; `onSave` returns whether the
 * snippet was accepted (the modal closes only then).
 */
interface DraftModeProps extends CommonConfigModalBaseProps {
  onSave: (value: string) => boolean | Promise<boolean>;
  onChange?: never;
}

/** Live mode: every edit is applied at once; Save only closes. */
interface LiveModeProps extends CommonConfigModalBaseProps {
  onChange: (value: string) => void;
  onSave?: never;
}

export type CommonConfigModalProps = DraftModeProps | LiveModeProps;

const isEmptySnippet = (value: string) => {
  const trimmed = value.trim();
  return trimmed === "" || trimmed === "{}";
};

/**
 * The one editor for an app's common-config snippet (Claude JSON, Codex TOML,
 * Gemini env JSON), replacing three copied modals (FE-PROV-18).
 */
export function CommonConfigModal(props: CommonConfigModalProps) {
  const {
    isOpen,
    onClose,
    title,
    hint,
    value,
    language,
    placeholder,
    error,
    onExtract,
    isExtracting,
    extractLabel,
  } = props;
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const isLive = typeof props.onChange === "function";
  const [draftValue, setDraftValue] = useState(value);

  useEffect(() => {
    if (isOpen) setDraftValue(value);
  }, [isOpen, value]);

  const shown = isLive ? value : draftValue;

  const handleChange = (next: string) => {
    if (props.onChange) props.onChange(next);
    else setDraftValue(next);
  };

  const handleClose = () => {
    if (!isLive) setDraftValue(value);
    onClose();
  };

  const handleSave = async () => {
    if (props.onSave) {
      if (await props.onSave(draftValue)) onClose();
      return;
    }
    onClose();
  };

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={title}
      onClose={handleClose}
      footer={
        <>
          {onExtract && (
            <Button
              type="button"
              variant="outline"
              onClick={onExtract}
              disabled={isExtracting}
              className="gap-2"
            >
              {isExtracting ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Download className="w-4 h-4" />
              )}
              {extractLabel ??
                t("codexConfig.extractFromCurrent", {
                  defaultValue: "Extract from Editor",
                })}
            </Button>
          )}
          <Button type="button" variant="outline" onClick={handleClose}>
            {t("common.cancel")}
          </Button>
          <Button type="button" onClick={handleSave} className="gap-2">
            <Save className="w-4 h-4" />
            {t("common.save")}
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <div className="rounded-lg border border-blue-200 dark:border-blue-800 bg-blue-50/50 dark:bg-blue-950/30 p-3 space-y-1.5">
          <p className="text-sm font-medium text-blue-800 dark:text-blue-300">
            {t("commonConfig.guideTitle")}
          </p>
          <p className="text-xs text-blue-700/80 dark:text-blue-400/80">
            {t("commonConfig.guidePurpose")}
          </p>
          <p className="text-xs text-blue-700/80 dark:text-blue-400/80">
            {t("commonConfig.guideUsage")}
          </p>
          <p className="text-xs text-blue-700/80 dark:text-blue-400/80">
            {t("commonConfig.guideReExtract")}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("commonConfig.guideReassurance")}
          </p>
        </div>
        {hint}
        {isEmptySnippet(shown) && (
          <div className="flex flex-col items-center justify-center py-6 text-center text-muted-foreground">
            <Package className="h-8 w-8 mb-2 opacity-40" />
            <p className="text-sm font-medium">
              {t("commonConfig.emptyTitle")}
            </p>
            <p className="text-xs mt-1">{t("commonConfig.emptyHint")}</p>
          </div>
        )}

        <JsonEditor
          value={shown}
          onChange={handleChange}
          placeholder={placeholder}
          darkMode={isDarkMode}
          rows={16}
          showValidation={true}
          language={language}
        />

        {error && (
          <p className="text-sm text-red-500 dark:text-red-400">{error}</p>
        )}
      </div>
    </FullScreenPanel>
  );
}
