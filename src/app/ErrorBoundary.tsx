import { Component, type ErrorInfo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  Copy,
  FolderOpen,
  RefreshCw,
  RotateCw,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { settingsApi } from "@/lib/api/settings";

interface ErrorBoundaryProps {
  children: ReactNode;
  /**
   * Changing this value clears a caught error (e.g. the current page id, so
   * navigating away from a crashed page recovers).
   */
  resetKey?: unknown;
  /** Compact in-page fallback instead of the full-window one. */
  variant?: "window" | "page";
}

interface ErrorBoundaryState {
  error: Error | null;
  componentStack: string | null;
  resetKey: unknown;
}

/**
 * Catches render errors so a crash shows a recovery screen instead of a
 * blank window (FE-ARCH-06).
 */
export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = {
    error: null,
    componentStack: null,
    resetKey: this.props.resetKey,
  };

  static getDerivedStateFromError(error: unknown): Partial<ErrorBoundaryState> {
    return {
      error: error instanceof Error ? error : new Error(String(error)),
    };
  }

  static getDerivedStateFromProps(
    props: ErrorBoundaryProps,
    state: ErrorBoundaryState,
  ): Partial<ErrorBoundaryState> | null {
    if (props.resetKey !== state.resetKey) {
      return { error: null, componentStack: null, resetKey: props.resetKey };
    }
    return null;
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error("[ErrorBoundary] Render error", error, info.componentStack);
    this.setState({ componentStack: info.componentStack ?? null });
  }

  private reset = () => {
    this.setState({ error: null, componentStack: null });
  };

  render() {
    if (!this.state.error) {
      return this.props.children;
    }
    return (
      <ErrorFallback
        error={this.state.error}
        componentStack={this.state.componentStack}
        onRetry={this.reset}
        variant={this.props.variant ?? "window"}
      />
    );
  }
}

interface ErrorFallbackProps {
  error: Error;
  componentStack: string | null;
  onRetry: () => void;
  variant: "window" | "page";
}

function buildDiagnostics(error: Error, componentStack: string | null) {
  return [
    `CC Switch render error`,
    `Time: ${new Date().toISOString()}`,
    `User agent: ${typeof navigator !== "undefined" ? navigator.userAgent : ""}`,
    ``,
    `${error.name}: ${error.message}`,
    error.stack ?? "",
    componentStack ? `\nComponent stack:${componentStack}` : "",
  ].join("\n");
}

function ErrorFallback({
  error,
  componentStack,
  onRetry,
  variant,
}: ErrorFallbackProps) {
  const { t } = useTranslation();

  const copyDiagnostics = () => {
    void navigator.clipboard
      ?.writeText(buildDiagnostics(error, componentStack))
      .catch(() => undefined);
  };

  return (
    <div
      role="alert"
      className={
        variant === "window"
          ? "flex h-screen w-full items-center justify-center bg-background p-6 text-foreground"
          : "flex flex-1 items-center justify-center p-6"
      }
    >
      <div className="max-w-lg space-y-4 text-center">
        <AlertTriangle className="mx-auto h-10 w-10 text-amber-500" />
        <h2 className="text-lg font-semibold">
          {t("errorBoundary.title", {
            defaultValue: "Something went wrong",
          })}
        </h2>
        <p className="text-sm text-muted-foreground">
          {t("errorBoundary.description", {
            defaultValue:
              "This part of CC Switch failed to render. Your configuration has not been changed.",
          })}
        </p>
        <pre className="max-h-40 overflow-auto rounded-md bg-muted p-3 text-left text-xs">
          {error.message || error.name}
        </pre>
        <div className="flex flex-wrap justify-center gap-2">
          <Button size="sm" onClick={onRetry}>
            <RefreshCw className="mr-2 h-4 w-4" />
            {t("errorBoundary.retry", { defaultValue: "Try again" })}
          </Button>
          <Button size="sm" variant="outline" onClick={copyDiagnostics}>
            <Copy className="mr-2 h-4 w-4" />
            {t("errorBoundary.copyDiagnostics", {
              defaultValue: "Copy diagnostics",
            })}
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() =>
              void settingsApi.openAppConfigFolder().catch(() => undefined)
            }
          >
            <FolderOpen className="mr-2 h-4 w-4" />
            {t("errorBoundary.openConfigFolder", {
              defaultValue: "Open config folder",
            })}
          </Button>
          {variant === "window" && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => void settingsApi.restart().catch(() => undefined)}
            >
              <RotateCw className="mr-2 h-4 w-4" />
              {t("errorBoundary.restart", { defaultValue: "Restart" })}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
