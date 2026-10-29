import type { ComponentProps } from "react";
import { useFormContext } from "react-hook-form";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import CodexConfigEditor from "@/components/providers/forms/CodexConfigEditor";
import { SettingsConfigErrorField } from "../SettingsConfigErrorField";

type CodexFormFieldsProps = ComponentProps<typeof CodexFormFields>;
type CodexConfigEditorProps = ComponentProps<typeof CodexConfigEditor>;

export interface CodexFieldsProps {
  fields: CodexFormFieldsProps;
  configEditor: Omit<CodexConfigEditorProps, "providerName">;
}

/** Codex 的专属字段与 auth.json / config.toml 编辑器 */
export function CodexFields({ fields, configEditor }: CodexFieldsProps) {
  const form = useFormContext<ProviderFormData>();
  return (
    <>
      <CodexFormFields {...fields} />
      <CodexConfigEditor {...configEditor} providerName={form.watch("name")} />
      <SettingsConfigErrorField />
    </>
  );
}
