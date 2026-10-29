import type { ComponentProps } from "react";
import { HermesFormFields } from "@/components/providers/forms/HermesFormFields";
import { SettingsConfigJsonField } from "../SettingsConfigJsonField";

type HermesFormFieldsProps = ComponentProps<typeof HermesFormFields>;

const HERMES_CONFIG_PLACEHOLDER = `{
  "name": "my-provider",
  "base_url": "https://api.example.com/v1",
  "api_key": ""
}`;

/** Hermes 的专属字段与 settingsConfig JSON 编辑器 */
export function HermesFields({ fields }: { fields: HermesFormFieldsProps }) {
  return (
    <>
      <HermesFormFields {...fields} />
      <SettingsConfigJsonField placeholder={HERMES_CONFIG_PLACEHOLDER} />
    </>
  );
}
