import { useTranslation } from "react-i18next";
import { AlertTriangle, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { extractErrorMessage } from "@/utils/errorUtils";

interface ProvidersErrorStateProps {
  error: unknown;
  onRetry: () => void;
  isRetrying?: boolean;
}

/**
 * Shown instead of the provider list when loading providers failed, so a
 * broken config/DB is not mistaken for "no providers yet" (FE-ARCH-06).
 */
export function ProvidersErrorState({
  error,
  onRetry,
  isRetrying,
}: ProvidersErrorStateProps) {
  const { t } = useTranslation();
  const detail = extractErrorMessage(error);

  return (
    <div
      role="alert"
      data-testid="providers-error"
      className="flex flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-red-500/40 p-10 text-center"
    >
      <AlertTriangle className="h-8 w-8 text-red-500" />
      <p className="text-sm font-medium">
        {t("provider.loadFailed", {
          defaultValue: "Failed to load providers",
        })}
      </p>
      {detail && (
        <p className="max-w-md break-words text-xs text-muted-foreground">
          {detail}
        </p>
      )}
      <Button size="sm" onClick={onRetry} disabled={isRetrying}>
        <RefreshCw
          className={`mr-2 h-4 w-4 ${isRetrying ? "animate-spin" : ""}`}
        />
        {t("common.retry", { defaultValue: "Retry" })}
      </Button>
    </div>
  );
}

/**
 * Whether the provider query should render its error state: it failed and
 * has no data of its own to show (placeholder data belongs to another app).
 */
export const shouldShowProvidersError = (query: {
  isError: boolean;
  data: unknown;
  isPlaceholderData: boolean;
}) => query.isError && (query.data === undefined || query.isPlaceholderData);
