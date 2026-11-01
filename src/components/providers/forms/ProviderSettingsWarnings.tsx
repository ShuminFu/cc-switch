import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { adaptersApi } from "@/lib/api/adapters";
import type { AppId } from "@/lib/api";
import type { JsonValue, ValidationIssue } from "@/lib/bindings";
import { queryKeys } from "@/lib/query/keys";

/** Apps the backend validator inspects (`adapters/validate.rs`). */
const VALIDATED_APPS: readonly AppId[] = ["claude", "codex", "gemini"];

const DEBOUNCE_MS = 400;

/** English fallbacks for the backend's issue keys (CFG-24). */
const DEFAULT_MESSAGES: Record<string, string> = {
  "validation.codex.wireApi.invalid":
    'wire_api "{{value}}" is not supported; use one of: {{allowed}}',
  "validation.codex.modelProvider.undefined":
    'model_provider "{{value}}" has no [model_providers.{{value}}] table',
  "validation.env.keyCase": '"{{value}}" should be written as "{{expected}}"',
  "validation.toml.syntax": "TOML syntax error: {{message}}",
};

export function formatValidationIssue(
  t: TFunction,
  issue: ValidationIssue,
): string {
  const raw =
    issue.params && typeof issue.params === "object" ? issue.params : {};
  const params: Record<string, string> = {};
  for (const [name, value] of Object.entries(raw as Record<string, unknown>)) {
    params[name] = Array.isArray(value)
      ? value.join(", ")
      : typeof value === "string"
        ? value
        : JSON.stringify(value);
  }
  return t(issue.key, {
    defaultValue: DEFAULT_MESSAGES[issue.key] ?? issue.key,
    ...params,
    interpolation: { escapeValue: false },
  }) as string;
}

interface ProviderSettingsWarningsProps {
  appId: AppId;
  /** The form's `settingsConfig` JSON text. */
  settingsConfig: string;
}

/**
 * Advisory warnings for the provider being edited, computed by the backend
 * (`validate_provider_settings`). Saving is never blocked by these.
 */
export function ProviderSettingsWarnings({
  appId,
  settingsConfig,
}: ProviderSettingsWarningsProps) {
  const { t } = useTranslation();
  const [debounced, setDebounced] = useState(settingsConfig);
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(settingsConfig), DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [settingsConfig]);

  let parsed: JsonValue | undefined;
  try {
    parsed = debounced.trim() ? (JSON.parse(debounced) as JsonValue) : {};
  } catch {
    // The JSON editor shows its own syntax error.
    parsed = undefined;
  }

  const enabled = VALIDATED_APPS.includes(appId) && parsed !== undefined;
  const { data: issues = [] } = useQuery({
    queryKey: queryKeys.adapters.validation(appId, debounced),
    queryFn: () => adaptersApi.validateProviderSettings(appId, parsed ?? {}),
    enabled,
    staleTime: Infinity,
    retry: false,
  });

  if (!enabled || issues.length === 0) return null;

  return (
    <div
      role="status"
      data-testid="provider-settings-warnings"
      className="space-y-1 rounded-lg border border-amber-500/40 bg-amber-500/10 p-3 text-sm text-amber-800 dark:text-amber-200"
    >
      {issues.map((issue, index) => (
        <p
          key={`${issue.path}:${issue.key}:${index}`}
          className="flex items-start gap-2"
        >
          <AlertTriangle className="mt-0.5 h-4 w-4 flex-shrink-0" />
          <span>
            <code className="mr-1 font-mono text-xs">{issue.path}</code>
            {formatValidationIssue(t, issue)}
          </span>
        </p>
      ))}
    </div>
  );
}
