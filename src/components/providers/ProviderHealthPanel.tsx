import { useMemo, useState } from "react";
import { Activity, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { HealthStatusIndicator } from "@/components/providers/HealthStatusIndicator";
import {
  streamCheckProvider,
  type StreamCheckResult,
} from "@/lib/api/connectivity-check";
import type { AppId } from "@/lib/api";
import { useProvidersHealthMap } from "@/lib/query/failover";
import type { Provider } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";

/** Providers the connectivity check can probe (official ones have no fixed endpoint). */
export const checkableProviders = (providers: Provider[]): Provider[] =>
  providers.filter((provider) => provider.category !== "official");

interface ProviderHealthPanelProps {
  open: boolean;
  appId: AppId;
  providers: Provider[];
  onOpenChange: (open: boolean) => void;
}

type CheckState =
  | { kind: "checking" }
  | { kind: "done"; result: StreamCheckResult }
  | { kind: "error"; message: string };

/**
 * Per-app health overview (FE-PROV-34, first increment): last connectivity
 * check with latency, and the proxy's failure counters per provider, with a
 * "Check all" that probes every non-official provider without a toast per
 * provider. Check results live for the panel session; history is not stored.
 */
export function ProviderHealthPanel({
  open,
  appId,
  providers,
  onOpenChange,
}: ProviderHealthPanelProps) {
  const { t, i18n } = useTranslation();
  const [checks, setChecks] = useState<Record<string, CheckState>>({});
  const ids = useMemo(() => providers.map((p) => p.id), [providers]);
  const { data: health = {} } = useProvidersHealthMap(appId, ids);
  const targets = useMemo(() => checkableProviders(providers), [providers]);
  const running = Object.values(checks).some((c) => c.kind === "checking");

  const checkOne = async (provider: Provider) => {
    setChecks((current) => ({
      ...current,
      [provider.id]: { kind: "checking" },
    }));
    try {
      const result = await streamCheckProvider(appId, provider.id);
      setChecks((current) => ({
        ...current,
        [provider.id]: { kind: "done", result },
      }));
    } catch (error) {
      setChecks((current) => ({
        ...current,
        [provider.id]: { kind: "error", message: extractErrorMessage(error) },
      }));
    }
  };

  const checkAll = () =>
    Promise.all(targets.map((provider) => checkOne(provider)));

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl" zIndex="nested">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Activity className="h-4 w-4" />
            {t("provider.health.title", { defaultValue: "Provider health" })}
          </DialogTitle>
          <DialogDescription>
            {t("provider.health.description", {
              defaultValue:
                "Reachability of each provider's endpoint and the failures the local route has seen.",
            })}
          </DialogDescription>
        </DialogHeader>

        <ul className="max-h-[55vh] divide-y divide-border/60 overflow-y-auto px-6 py-2 text-sm">
          {providers.map((provider) => {
            const check = checks[provider.id];
            const stats = health[provider.id];
            const checkable = provider.category !== "official";
            return (
              <li
                key={provider.id}
                className="flex items-center gap-3 py-2"
                data-testid={`health-row-${provider.id}`}
              >
                <span className="min-w-0 flex-1 truncate font-medium">
                  {provider.name}
                </span>
                <span className="w-40 shrink-0 text-xs">
                  {!checkable ? (
                    <span className="text-muted-foreground">
                      {t("provider.health.notCheckable", {
                        defaultValue: "Official: not probed",
                      })}
                    </span>
                  ) : check?.kind === "checking" ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin text-muted-foreground" />
                  ) : check?.kind === "done" ? (
                    <HealthStatusIndicator
                      status={check.result.status}
                      responseTimeMs={check.result.responseTimeMs}
                    />
                  ) : check?.kind === "error" ? (
                    <span
                      className="text-red-600 dark:text-red-400"
                      title={check.message}
                    >
                      {t("provider.health.checkFailed", {
                        defaultValue: "Check failed",
                      })}
                    </span>
                  ) : (
                    <span className="text-muted-foreground">
                      {t("provider.health.notChecked", {
                        defaultValue: "Not checked yet",
                      })}
                    </span>
                  )}
                </span>
                <span
                  className="w-44 shrink-0 text-right text-xs text-muted-foreground"
                  title={stats?.last_error ?? undefined}
                >
                  {stats
                    ? stats.consecutive_failures > 0
                      ? t("provider.health.failures", {
                          defaultValue: "{{count}} failures in a row",
                          count: stats.consecutive_failures,
                        })
                      : stats.last_success_at
                        ? t("provider.health.lastSuccess", {
                            defaultValue: "OK at {{time}}",
                            time: new Date(
                              stats.last_success_at,
                            ).toLocaleTimeString(i18n.language),
                          })
                        : t("provider.health.noTraffic", {
                            defaultValue: "No routed requests",
                          })
                    : t("provider.health.noTraffic", {
                        defaultValue: "No routed requests",
                      })}
                </span>
              </li>
            );
          })}
        </ul>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.close", { defaultValue: "Close" })}
          </Button>
          <Button
            onClick={() => void checkAll()}
            disabled={running || targets.length === 0}
          >
            {running && <Loader2 className="mr-1 h-4 w-4 animate-spin" />}
            {t("provider.health.checkAll", {
              defaultValue: "Check all ({{count}})",
              count: targets.length,
            })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
