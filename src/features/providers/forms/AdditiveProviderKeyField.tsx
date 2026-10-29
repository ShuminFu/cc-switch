import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

/** opencode / openclaw / hermes 的 providerKey 格式：小写字母数字，连字符分隔 */
export const PROVIDER_KEY_PATTERN = /^[a-z0-9]+(-[a-z0-9]+)*$/;

/** 各累加应用 providerKey 相关文案所在的 i18n 命名空间 */
export type ProviderKeyI18nPrefix = "opencode" | "openclaw" | "hermes.form";

export const providerKeyMessageKeys = (prefix: ProviderKeyI18nPrefix) =>
  ({
    label: `${prefix}.providerKey`,
    placeholder: `${prefix}.providerKeyPlaceholder`,
    hint: `${prefix}.providerKeyHint`,
    lockedHint: `${prefix}.providerKeyLockedHint`,
    required: `${prefix}.providerKeyRequired`,
    invalid: `${prefix}.providerKeyInvalid`,
    duplicate: `${prefix}.providerKeyDuplicate`,
  }) as const;

interface AdditiveProviderKeyFieldProps {
  inputId: string;
  i18nPrefix: ProviderKeyI18nPrefix;
  value: string;
  onChange: (value: string) => void;
  /** 已写入 live 配置的 key 不可再改 */
  isLocked: boolean;
  isLockStateLoading: boolean;
  existingKeys: string[];
}

/** 累加应用（opencode / openclaw / hermes）的 providerKey 输入框 */
export function AdditiveProviderKeyField({
  inputId,
  i18nPrefix,
  value,
  onChange,
  isLocked,
  isLockStateLoading,
  existingKeys,
}: AdditiveProviderKeyFieldProps) {
  const { t } = useTranslation();
  const keys = providerKeyMessageKeys(i18nPrefix);
  const isDuplicate = existingKeys.includes(value) && !isLocked;
  const isInvalid = value.trim() !== "" && !PROVIDER_KEY_PATTERN.test(value);

  return (
    <div className="space-y-2">
      <Label htmlFor={inputId}>
        {t(keys.label)}
        <span className="text-destructive ml-1">*</span>
      </Label>
      <Input
        id={inputId}
        value={value}
        onChange={(e) =>
          onChange(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ""))
        }
        placeholder={t(keys.placeholder)}
        disabled={isLocked || isLockStateLoading}
        className={isDuplicate || isInvalid ? "border-destructive" : ""}
      />
      {isDuplicate && (
        <p className="text-xs text-destructive">{t(keys.duplicate)}</p>
      )}
      {isInvalid && (
        <p className="text-xs text-destructive">{t(keys.invalid)}</p>
      )}
      {!isDuplicate && !isInvalid && (
        <p className="text-xs text-muted-foreground">
          {isLocked ? t(keys.lockedHint) : t(keys.hint)}
        </p>
      )}
    </div>
  );
}
