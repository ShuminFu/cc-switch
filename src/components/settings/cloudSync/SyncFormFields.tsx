import { Info } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { CloudSyncBackendAdapter, SyncFormValues } from "./types";

interface SyncFormFieldsProps<F extends SyncFormValues> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  adapter: CloudSyncBackendAdapter<F, any>;
  form: F;
  disabled: boolean;
  onChange: (name: keyof F & string, value: string | boolean) => void;
  onBlur: (name: keyof F & string) => void;
  onPresetChange: (presetId: string) => void;
}

function FieldLabel({ label, hint }: { label: string; hint?: string }) {
  const { t } = useTranslation();
  return (
    <label className="w-40 text-xs font-medium text-foreground shrink-0">
      {t(label)}
      {hint && (
        <span className="block text-[10px] font-normal text-muted-foreground">
          {t(hint)}
        </span>
      )}
    </label>
  );
}

/** Preset selector plus the adapter's field schema, rendered in order. */
export function SyncFormFields<F extends SyncFormValues>({
  adapter,
  form,
  disabled,
  onChange,
  onBlur,
  onPresetChange,
}: SyncFormFieldsProps<F>) {
  const { t } = useTranslation();
  const activePreset = adapter.presets.find((p) => p.id === form.preset);

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-4">
        <FieldLabel label={adapter.messages.presetsLabel} />
        <Select
          value={form.preset}
          onValueChange={onPresetChange}
          disabled={disabled}
        >
          <SelectTrigger className="text-xs flex-1">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {adapter.presets.map((preset) => (
              <SelectItem key={preset.id} value={preset.id}>
                {t(preset.label)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {adapter.fields.map((field) => {
        if (field === "presetHint") {
          return activePreset?.hint ? (
            <div
              key="presetHint"
              className="flex items-start gap-2 pl-44 text-xs text-muted-foreground"
            >
              <Info className="h-3.5 w-3.5 shrink-0 mt-0.5" />
              <span>{t(activePreset.hint)}</span>
            </div>
          ) : null;
        }

        if (field.kind === "switch") {
          return (
            <div key={field.name} className="flex items-start gap-4">
              <FieldLabel label={field.label} hint={field.hint} />
              <div className="pt-1">
                <Switch
                  checked={Boolean(form[field.name])}
                  onCheckedChange={(checked) => onChange(field.name, checked)}
                  aria-label={t(field.label)}
                  disabled={disabled}
                />
              </div>
            </div>
          );
        }

        return (
          <div key={field.name} className="flex items-center gap-4">
            <FieldLabel label={field.label} hint={field.hint} />
            <Input
              type={field.kind === "password" ? "password" : undefined}
              value={String(form[field.name] ?? "")}
              onChange={(e) => onChange(field.name, e.target.value)}
              onBlur={field.onBlur ? () => onBlur(field.name) : undefined}
              placeholder={field.placeholder?.(t, form)}
              className="text-xs flex-1"
              autoComplete={field.kind === "password" ? "off" : undefined}
              disabled={disabled}
            />
          </div>
        );
      })}
    </div>
  );
}
