import { useFormContext } from "react-hook-form";
import { FormField, FormItem, FormMessage } from "@/components/ui/form";
import type { ProviderFormData } from "@/lib/schemas/provider";

/** settingsConfig 的 schema 错误（JSON 解析失败等），显示在配置编辑器下方 */
export function SettingsConfigErrorField() {
  const { control } = useFormContext<ProviderFormData>();
  return (
    <FormField
      control={control}
      name="settingsConfig"
      render={() => (
        <FormItem className="space-y-0">
          <FormMessage />
        </FormItem>
      )}
    />
  );
}
