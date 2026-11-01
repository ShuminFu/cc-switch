import { useTranslation } from "react-i18next";
import { AlertCircle, RotateCw } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { extractErrorMessage } from "@/utils/errorUtils";

interface ConfigLoadErrorAlertProps {
  error: unknown;
  onRetry?: () => void;
}

/** Inline error shown when a config panel could not load its backend state. */
export function ConfigLoadErrorAlert({
  error,
  onRetry,
}: ConfigLoadErrorAlertProps) {
  const { t } = useTranslation();
  const detail = extractErrorMessage(error) || String(error);
  return (
    <Alert variant="destructive" data-testid="config-load-error">
      <AlertCircle className="h-4 w-4" />
      <AlertDescription className="flex items-center justify-between gap-3">
        <span>
          {t("settings.configLoadFailed", {
            defaultValue:
              "Could not load the current settings; editing is disabled. {{detail}}",
            detail,
          })}
        </span>
        {onRetry && (
          <Button size="sm" variant="outline" onClick={onRetry}>
            <RotateCw className="mr-1 h-3.5 w-3.5" />
            {t("common.retry", { defaultValue: "Retry" })}
          </Button>
        )}
      </AlertDescription>
    </Alert>
  );
}
