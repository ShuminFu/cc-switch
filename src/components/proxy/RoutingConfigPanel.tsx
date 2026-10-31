import { useEffect, useState } from "react";
import { Loader2, Save, Shuffle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { SegmentedControl } from "@/components/common/SegmentedControl";
import { HelpLink } from "@/components/common/HelpLink";
import { useRoutingConfig, useSetRoutingConfig } from "@/lib/query/proxy";
import { useFailoverQueue } from "@/lib/query/failover";
import { showError } from "@/lib/errors/showError";
import type { RoutingConfig, RoutingStrategy } from "@/types/proxy";

interface RoutingConfigPanelProps {
  appType: string;
  disabled?: boolean;
}

const toCount = (raw: string): number => {
  const value = Number.parseInt(raw, 10);
  return Number.isFinite(value) && value > 0 ? value : 0;
};

/**
 * Per-app routing (PROXY-19): which queued provider a request tries first,
 * sticky sessions and a per-provider concurrency cap. Every default keeps
 * today's fixed priority order.
 */
export function RoutingConfigPanel({
  appType,
  disabled = false,
}: RoutingConfigPanelProps) {
  const { t } = useTranslation();
  const { data: saved, isLoading } = useRoutingConfig(appType);
  const { data: queue = [] } = useFailoverQueue(appType);
  const save = useSetRoutingConfig(appType);
  const [draft, setDraft] = useState<RoutingConfig | null>(null);

  useEffect(() => {
    if (saved) setDraft(saved);
  }, [saved]);

  if (isLoading || !draft) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" />
        {t("common.loading")}
      </div>
    );
  }

  const update = (patch: Partial<RoutingConfig>) =>
    setDraft((current) => (current ? { ...current, ...patch } : current));
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);

  const handleSave = async () => {
    try {
      await save.mutateAsync(draft);
      toast.success(
        t("proxy.routing.saved", { defaultValue: "Routing settings saved" }),
        { closeButton: true },
      );
    } catch (error) {
      showError({
        title: t("proxy.routing.saveFailed", {
          defaultValue: "Failed to save routing settings",
        }),
        error,
      });
    }
  };

  const strategies: { value: RoutingStrategy; label: string }[] = [
    {
      value: "priority",
      label: t("proxy.routing.strategy.priority", {
        defaultValue: "Priority",
      }),
    },
    {
      value: "roundRobin",
      label: t("proxy.routing.strategy.roundRobin", {
        defaultValue: "Round robin",
      }),
    },
    {
      value: "weighted",
      label: t("proxy.routing.strategy.weighted", {
        defaultValue: "Weighted",
      }),
    },
  ];

  const stickyId = `routing-sticky-${appType}`;
  const ttlId = `routing-sticky-ttl-${appType}`;
  const concurrencyId = `routing-concurrency-${appType}`;

  return (
    <section className="space-y-4" data-testid={`routing-config-${appType}`}>
      <div>
        <h4 className="flex items-center gap-2 text-sm font-semibold">
          <Shuffle className="h-4 w-4" />
          {t("proxy.routing.title", { defaultValue: "Routing" })}
          <HelpLink topic="proxy.routing" />
        </h4>
        <p className="text-xs text-muted-foreground">
          {t("proxy.routing.description", {
            defaultValue:
              "Which provider in the queue a request tries first. Failover still walks the rest of the queue.",
          })}
        </p>
      </div>

      <SegmentedControl
        ariaLabel={t("proxy.routing.strategyLabel", {
          defaultValue: "Routing strategy",
        })}
        options={strategies}
        value={draft.strategy}
        onChange={(strategy) => update({ strategy })}
        disabled={disabled}
      />

      {draft.strategy === "weighted" && queue.length > 0 && (
        <div className="space-y-2">
          <p className="text-xs text-muted-foreground">
            {t("proxy.routing.weightsHint", {
              defaultValue:
                "Higher weights are tried first more often. 0 means never first.",
            })}
          </p>
          {queue.map((item) => {
            const weightId = `routing-weight-${appType}-${item.providerId}`;
            return (
              <div key={item.providerId} className="flex items-center gap-3">
                <Label htmlFor={weightId} className="flex-1 truncate text-xs">
                  {item.providerName}
                </Label>
                <Input
                  id={weightId}
                  type="number"
                  min={0}
                  className="h-8 w-24 text-xs"
                  disabled={disabled}
                  value={draft.weights[item.providerId] ?? 1}
                  onChange={(event) =>
                    update({
                      weights: {
                        ...draft.weights,
                        [item.providerId]: toCount(event.target.value),
                      },
                    })
                  }
                />
              </div>
            );
          })}
        </div>
      )}

      <div className="flex items-center justify-between gap-3">
        <div className="space-y-0.5">
          <Label htmlFor={stickyId} className="text-sm">
            {t("proxy.routing.sticky", { defaultValue: "Sticky sessions" })}
          </Label>
          <p className="text-xs text-muted-foreground">
            {t("proxy.routing.stickyHint", {
              defaultValue:
                "Keep a conversation on the provider that served its first request, so prompt caches stay warm.",
            })}
          </p>
        </div>
        <Switch
          id={stickyId}
          checked={draft.stickySessions}
          disabled={disabled}
          onCheckedChange={(stickySessions) => update({ stickySessions })}
        />
      </div>

      {draft.stickySessions && (
        <div className="flex items-center gap-3">
          <Label htmlFor={ttlId} className="flex-1 text-xs">
            {t("proxy.routing.stickyTtl", {
              defaultValue:
                "Pin lifetime after last use (seconds, 0 = default)",
            })}
          </Label>
          <Input
            id={ttlId}
            type="number"
            min={0}
            className="h-8 w-28 text-xs"
            disabled={disabled}
            value={draft.stickyTtlSecs}
            onChange={(event) =>
              update({ stickyTtlSecs: toCount(event.target.value) })
            }
          />
        </div>
      )}

      <div className="flex items-center gap-3">
        <Label htmlFor={concurrencyId} className="flex-1 text-xs">
          {t("proxy.routing.maxConcurrent", {
            defaultValue:
              "Max in-flight requests per provider (0 = unlimited); busy providers are skipped",
          })}
        </Label>
        <Input
          id={concurrencyId}
          type="number"
          min={0}
          className="h-8 w-28 text-xs"
          disabled={disabled}
          value={draft.maxConcurrentPerProvider}
          onChange={(event) =>
            update({ maxConcurrentPerProvider: toCount(event.target.value) })
          }
        />
      </div>

      <div className="flex justify-end">
        <Button
          size="sm"
          onClick={() => void handleSave()}
          disabled={disabled || !dirty || save.isPending}
        >
          {save.isPending ? (
            <Loader2 className="mr-1 h-4 w-4 animate-spin" />
          ) : (
            <Save className="mr-1 h-4 w-4" />
          )}
          {t("common.save")}
        </Button>
      </div>
    </section>
  );
}
