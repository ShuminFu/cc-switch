import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { sanitizeProviderKeyInput } from "./providerKey";
import type { AdditiveProviderKeyState } from "./useAdditiveProviderKey";

interface ProviderKeyFieldProps {
  /** DOM id of the input (`<app>-key`). */
  id: string;
  value: string;
  onChange: (key: string) => void;
  state: AdditiveProviderKeyState;
}

const LOCKED_HINT_DEFAULTS: Record<string, string> = {
  opencode: "该供应商已添加到应用配置中，供应商标识不可修改",
  openclaw: "该供应商已添加到应用配置中，供应商标识不可修改",
  "hermes.form": "This provider is in Hermes config; key is locked.",
};

/**
 * Provider-key input shared by the OpenCode / OpenClaw / Hermes adapters
 * (FE-PROV-52); it used to be copy-pasted three times in ProviderForm.
 */
export function ProviderKeyField({
  id,
  value,
  onChange,
  state,
}: ProviderKeyFieldProps) {
  const { t } = useTranslation();
  const ns = state.namespace;
  const isHermes = ns === "hermes.form";
  const hasError = state.isDuplicate || state.isFormatInvalid;
  const hintId = `${id}-hint`;

  return (
    <div className="space-y-2">
      <Label htmlFor={id}>
        {t(
          `${ns}.providerKey`,
          isHermes ? { defaultValue: "Provider Key" } : {},
        )}
        <span className="text-destructive ml-1" aria-hidden="true">
          *
        </span>
      </Label>
      <Input
        id={id}
        value={value}
        onChange={(event) =>
          onChange(sanitizeProviderKeyInput(event.target.value))
        }
        placeholder={t(
          `${ns}.providerKeyPlaceholder`,
          isHermes ? { defaultValue: "my-provider" } : {},
        )}
        disabled={state.isLocked || state.isStatusLoading}
        aria-required="true"
        aria-invalid={hasError || undefined}
        aria-describedby={hintId}
        className={hasError ? "border-destructive" : ""}
      />
      {state.isDuplicate ? (
        <p id={hintId} className="text-xs text-destructive">
          {t(`${ns}.providerKeyDuplicate`)}
        </p>
      ) : state.isFormatInvalid ? (
        <p id={hintId} className="text-xs text-destructive">
          {t(`${ns}.providerKeyInvalid`)}
        </p>
      ) : (
        <p id={hintId} className="text-xs text-muted-foreground">
          {state.isLocked
            ? t(`${ns}.providerKeyLockedHint`, {
                defaultValue: LOCKED_HINT_DEFAULTS[ns],
              })
            : t(
                `${ns}.providerKeyHint`,
                isHermes
                  ? {
                      defaultValue:
                        "Lowercase letters, numbers, and hyphens only. Used as the provider name in config.yaml.",
                    }
                  : {},
              )}
        </p>
      )}
    </div>
  );
}
