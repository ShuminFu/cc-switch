import type { ComponentProps } from "react";
import { useFormContext } from "react-hook-form";
import { useTranslation } from "react-i18next";
import {
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import JsonEditor from "@/components/JsonEditor";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { useDarkMode } from "@/hooks/useDarkMode";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { validateGrokBuildConfig } from "@/utils/grokBuildConfig";

type CodexFormFieldsProps = ComponentProps<typeof CodexFormFields>;

export interface GrokBuildFieldsProps {
  profile: string;
  onProfileChange: (value: string) => void;
  apiBackend: string;
  onApiBackendChange: (value: string) => void;
  contextWindow: string;
  onContextWindowChange: (value: string) => void;
  /** 端点、API Key、上游模型与代理层选项沿用 Codex 的字段组件 */
  codexFields: Omit<CodexFormFieldsProps, "appId">;
  rawConfig: string;
  onRawConfigChange: (value: string) => void;
}

/** Grok Build 的 profile 字段、Codex 风格的上游字段与 config.toml 编辑器 */
export function GrokBuildFields({
  profile,
  onProfileChange,
  apiBackend,
  onApiBackendChange,
  contextWindow,
  onContextWindowChange,
  codexFields,
  rawConfig,
  onRawConfigChange,
}: GrokBuildFieldsProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const { control } = useFormContext<ProviderFormData>();
  const rawConfigError = validateGrokBuildConfig(rawConfig);

  return (
    <>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        <FormItem>
          <FormLabel htmlFor="grokbuild-profile">
            {t("grokBuild.profile")}
          </FormLabel>
          <Input
            id="grokbuild-profile"
            value={profile}
            onChange={(event) => onProfileChange(event.target.value)}
            placeholder="grok-4.5"
            autoComplete="off"
          />
        </FormItem>

        <FormItem>
          <FormLabel htmlFor="grokbuild-api-backend">
            {t("grokBuild.apiBackend")}
          </FormLabel>
          <Input
            id="grokbuild-api-backend"
            value={apiBackend}
            onChange={(event) => onApiBackendChange(event.target.value)}
            placeholder="responses"
            autoComplete="off"
          />
        </FormItem>

        <FormItem>
          <FormLabel htmlFor="grokbuild-context-window">
            {t("grokBuild.contextWindow")}
          </FormLabel>
          <Input
            id="grokbuild-context-window"
            type="number"
            min={1}
            step={1}
            value={contextWindow}
            onChange={(event) => onContextWindowChange(event.target.value)}
          />
        </FormItem>
      </div>

      <CodexFormFields appId="grokbuild" {...codexFields} />

      <div className="space-y-2">
        <FormLabel htmlFor="grokbuild-config-toml">
          {t("grokBuild.rawConfig")}
        </FormLabel>
        <JsonEditor
          value={rawConfig}
          onChange={onRawConfigChange}
          placeholder=""
          darkMode={isDarkMode}
          rows={12}
          showValidation={false}
          language="javascript"
        />
        {rawConfigError && (
          <p className="text-xs text-destructive">
            {t("grokBuild.invalidToml", {
              error: rawConfigError,
            })}
          </p>
        )}
      </div>

      <FormField
        control={control}
        name="settingsConfig"
        render={() => (
          <FormItem className="hidden">
            <FormControl>
              <Input type="hidden" />
            </FormControl>
            <FormMessage />
          </FormItem>
        )}
      />
    </>
  );
}
