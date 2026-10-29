import { useTranslation } from "react-i18next";
import { useFormContext } from "react-hook-form";
import JsonEditor from "@/components/JsonEditor";
import { Label } from "@/components/ui/label";
import { useDarkMode } from "@/hooks/useDarkMode";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { SettingsConfigErrorField } from "./SettingsConfigErrorField";

/** settingsConfig 的 JSON 编辑器及其 schema 错误（累加应用共用） */
export function SettingsConfigJsonField({
  placeholder,
}: {
  placeholder: string;
}) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const form = useFormContext<ProviderFormData>();
  return (
    <>
      <div className="space-y-2">
        <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
        <JsonEditor
          value={form.getValues("settingsConfig")}
          onChange={(config) => form.setValue("settingsConfig", config)}
          placeholder={placeholder}
          rows={14}
          showValidation={true}
          language="json"
          darkMode={isDarkMode}
        />
      </div>
      <SettingsConfigErrorField />
    </>
  );
}
