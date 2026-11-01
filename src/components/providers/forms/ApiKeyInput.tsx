import React, { useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { useTranslation } from "react-i18next";
import { normalizeApiKey } from "@/utils/providerInputValidation";

interface ApiKeyInputProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  required?: boolean;
  label?: string;
  id?: string;
}

const ApiKeyInput: React.FC<ApiKeyInputProps> = ({
  value,
  onChange,
  placeholder,
  disabled = false,
  required = false,
  label = "API Key",
  id = "apiKey",
}) => {
  const { t } = useTranslation();
  const [showKey, setShowKey] = useState(false);

  const toggleShowKey = () => {
    setShowKey(!showKey);
  };

  const inputClass = `w-full px-3 py-2 pr-10 border rounded-lg text-sm transition-colors ${
    disabled
      ? "bg-muted border-border-default text-muted-foreground cursor-not-allowed"
      : "border-border-default bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20"
  }`;

  return (
    <div className="space-y-2">
      <label htmlFor={id} className="block text-sm font-medium text-foreground">
        {label} {required && "*"}
      </label>
      <div className="relative">
        <input
          type={showKey ? "text" : "password"}
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onPaste={(e) => {
            // Strip quotes, a "Bearer " prefix and line breaks that often
            // come along with a copied key (FE-PROV-36).
            const pasted = e.clipboardData.getData("text");
            const normalized = normalizeApiKey(pasted);
            if (normalized === pasted) return;
            e.preventDefault();
            const target = e.currentTarget;
            const start = target.selectionStart ?? value.length;
            const end = target.selectionEnd ?? value.length;
            onChange(value.slice(0, start) + normalized + value.slice(end));
          }}
          placeholder={placeholder ?? t("apiKeyInput.placeholder")}
          disabled={disabled}
          aria-required={required || undefined}
          autoComplete="off"
          className={inputClass}
        />
        {!disabled && value && (
          <button
            type="button"
            onClick={toggleShowKey}
            className="absolute inset-y-0 right-0 flex items-center pr-3 text-muted-foreground hover:text-foreground transition-colors"
            aria-label={showKey ? t("apiKeyInput.hide") : t("apiKeyInput.show")}
          >
            {showKey ? <EyeOff size={16} /> : <Eye size={16} />}
          </button>
        )}
      </div>
    </div>
  );
};

export default ApiKeyInput;
