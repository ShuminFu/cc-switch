import type { ComponentProps } from "react";
import { useFormContext } from "react-hook-form";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory } from "@/types";
import { hasApiKeyField } from "@/utils/providerConfigUtils";
import { ClaudeFormFields } from "@/components/providers/forms/ClaudeFormFields";
import { CommonConfigEditor } from "@/components/providers/forms/CommonConfigEditor";
import { SettingsConfigErrorField } from "../SettingsConfigErrorField";

type ClaudeFormFieldsProps = ComponentProps<typeof ClaudeFormFields>;
type CommonConfigEditorProps = ComponentProps<typeof CommonConfigEditor>;

export interface ClaudeFieldsProps {
  category?: ProviderCategory;
  isEditMode: boolean;
  /** useApiKeyState 的 showApiKey：按当前 settingsConfig 判断是否显示 API Key */
  showApiKey: (settingsConfig: string, isEditMode: boolean) => boolean;
  fields: Omit<ClaudeFormFieldsProps, "shouldShowApiKey" | "category">;
  commonConfig: Omit<CommonConfigEditorProps, "value" | "onChange">;
}

/** Claude 的专属字段与 settings.json 编辑器；settingsConfig 通过 useFormContext 读写 */
export function ClaudeFields({
  category,
  isEditMode,
  showApiKey,
  fields,
  commonConfig,
}: ClaudeFieldsProps) {
  const form = useFormContext<ProviderFormData>();
  const settingsConfig = form.getValues("settingsConfig");

  return (
    <>
      <ClaudeFormFields
        {...fields}
        category={category}
        shouldShowApiKey={
          (category !== "cloud_provider" ||
            hasApiKeyField(settingsConfig, "claude")) &&
          showApiKey(settingsConfig, isEditMode)
        }
      />

      <CommonConfigEditor
        {...commonConfig}
        value={settingsConfig}
        onChange={(value) => form.setValue("settingsConfig", value)}
      />
      <SettingsConfigErrorField />
    </>
  );
}
