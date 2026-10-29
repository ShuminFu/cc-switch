import type { ComponentProps } from "react";
import { useFormContext } from "react-hook-form";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory } from "@/types";
import { GeminiFormFields } from "@/components/providers/forms/GeminiFormFields";
import GeminiConfigEditor from "@/components/providers/forms/GeminiConfigEditor";
import { SettingsConfigErrorField } from "../SettingsConfigErrorField";

type GeminiFormFieldsProps = ComponentProps<typeof GeminiFormFields>;
type GeminiConfigEditorProps = ComponentProps<typeof GeminiConfigEditor>;

export interface GeminiFieldsProps {
  category?: ProviderCategory;
  isEditMode: boolean;
  /** useApiKeyState 的 showApiKey：按当前 settingsConfig 判断是否显示 API Key */
  showApiKey: (settingsConfig: string, isEditMode: boolean) => boolean;
  fields: Omit<GeminiFormFieldsProps, "shouldShowApiKey" | "category">;
  configEditor: GeminiConfigEditorProps;
}

/** Gemini 的专属字段与 .env / 扩展配置编辑器 */
export function GeminiFields({
  category,
  isEditMode,
  showApiKey,
  fields,
  configEditor,
}: GeminiFieldsProps) {
  const form = useFormContext<ProviderFormData>();
  return (
    <>
      <GeminiFormFields
        {...fields}
        category={category}
        shouldShowApiKey={showApiKey(
          form.getValues("settingsConfig"),
          isEditMode,
        )}
      />
      <GeminiConfigEditor {...configEditor} />
      <SettingsConfigErrorField />
    </>
  );
}
