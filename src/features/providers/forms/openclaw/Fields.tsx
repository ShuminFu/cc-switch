import type { ComponentProps } from "react";
import { OpenClawFormFields } from "@/components/providers/forms/OpenClawFormFields";
import { SettingsConfigJsonField } from "../SettingsConfigJsonField";

type OpenClawFormFieldsProps = ComponentProps<typeof OpenClawFormFields>;

const OPENCLAW_CONFIG_PLACEHOLDER = `{
  "baseUrl": "https://api.example.com/v1",
  "apiKey": "your-api-key-here",
  "api": "openai-completions",
  "models": []
}`;

/** OpenClaw 的专属字段与 settingsConfig JSON 编辑器 */
export function OpenClawFields({
  fields,
}: {
  fields: OpenClawFormFieldsProps;
}) {
  return (
    <>
      <OpenClawFormFields {...fields} />
      <SettingsConfigJsonField placeholder={OPENCLAW_CONFIG_PLACEHOLDER} />
    </>
  );
}
