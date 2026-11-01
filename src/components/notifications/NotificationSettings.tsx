import { useTranslation } from "react-i18next";
import {
  Bell,
  ArrowRightLeft,
  ShieldAlert,
  Wallet,
  Gauge,
  CloudOff,
  Activity,
} from "lucide-react";
import { ToggleRow } from "@/components/ui/toggle-row";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { SettingsFormState } from "@/hooks/useSettings";
import type { NotificationToggles } from "@/types";
import {
  HEALTH_PROBE_INTERVALS,
  resolveHealthProbe,
  resolveNotificationToggles,
} from "./notificationSettings";

interface NotificationSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

/**
 * Settings → General: per-category system notification toggles and the
 * opt-in background health prober for failover-queue providers.
 */
export function NotificationSettings({
  settings,
  onChange,
}: NotificationSettingsProps) {
  const { t } = useTranslation();
  const toggles = resolveNotificationToggles(settings.notifications);
  const probe = resolveHealthProbe(settings.healthProbe);

  const setToggle = (key: keyof NotificationToggles, value: boolean) =>
    onChange({ notifications: { ...toggles, [key]: value } });

  const rows: Array<{
    key: keyof NotificationToggles;
    icon: React.ReactNode;
    title: string;
    description: string;
  }> = [
    {
      key: "failover",
      icon: <ArrowRightLeft className="h-4 w-4 text-blue-500" />,
      title: t("settings.notifications.failover", {
        defaultValue: "Failover switches",
      }),
      description: t("settings.notifications.failoverDescription", {
        defaultValue: "Notify when the proxy fails over to another provider",
      }),
    },
    {
      key: "circuitOpen",
      icon: <ShieldAlert className="h-4 w-4 text-red-500" />,
      title: t("settings.notifications.circuitOpen", {
        defaultValue: "Circuit breaker opened",
      }),
      description: t("settings.notifications.circuitOpenDescription", {
        defaultValue:
          "Notify when a provider is paused after repeated failures",
      }),
    },
    {
      key: "budgetExceeded",
      icon: <Wallet className="h-4 w-4 text-amber-500" />,
      title: t("settings.notifications.budgetExceeded", {
        defaultValue: "Budget limit reached",
      }),
      description: t("settings.notifications.budgetExceededDescription", {
        defaultValue:
          "Notify when a provider exceeds its daily or monthly limit",
      }),
    },
    {
      key: "quotaExhausted",
      icon: <Gauge className="h-4 w-4 text-purple-500" />,
      title: t("settings.notifications.quotaExhausted", {
        defaultValue: "Subscription quota exhausted",
      }),
      description: t("settings.notifications.quotaExhaustedDescription", {
        defaultValue: "Notify when a subscription usage window reaches 100%",
      }),
    },
    {
      key: "syncError",
      icon: <CloudOff className="h-4 w-4 text-gray-500" />,
      title: t("settings.notifications.syncError", {
        defaultValue: "Sync errors",
      }),
      description: t("settings.notifications.syncErrorDescription", {
        defaultValue: "Notify when WebDAV or S3 auto sync fails",
      }),
    },
  ];

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <Bell className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">
          {t("settings.notifications.title", {
            defaultValue: "System notifications",
          })}
        </h3>
      </div>
      <p className="text-xs text-muted-foreground">
        {t("settings.notifications.hint", {
          defaultValue:
            "Shown even when the window is closed. Each provider triggers at most one notification per category every 5 minutes.",
        })}
      </p>

      <div className="space-y-3">
        {rows.map((row) => (
          <ToggleRow
            key={row.key}
            icon={row.icon}
            title={row.title}
            description={row.description}
            checked={toggles[row.key]}
            onCheckedChange={(value) => setToggle(row.key, value)}
          />
        ))}

        <ToggleRow
          icon={<Activity className="h-4 w-4 text-green-500" />}
          title={t("settings.healthProbe.title", {
            defaultValue: "Background health probing",
          })}
          description={t("settings.healthProbe.description", {
            defaultValue:
              "While the proxy runs, periodically send a lightweight request to failover-queue providers whose circuit is open, so they recover without using one of your requests",
          })}
          checked={probe.enabled}
          onCheckedChange={(value) =>
            onChange({ healthProbe: { ...probe, enabled: value } })
          }
        />

        {probe.enabled ? (
          <div className="flex items-center justify-between gap-4 rounded-xl border border-border bg-card/50 p-4">
            <p className="text-sm font-medium leading-none">
              {t("settings.healthProbe.interval", {
                defaultValue: "Probe interval",
              })}
            </p>
            <Select
              value={String(probe.intervalSecs)}
              onValueChange={(value) =>
                onChange({
                  healthProbe: { ...probe, intervalSecs: Number(value) },
                })
              }
            >
              <SelectTrigger
                className="w-40"
                aria-label={t("settings.healthProbe.interval", {
                  defaultValue: "Probe interval",
                })}
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {(HEALTH_PROBE_INTERVALS as readonly number[])
                  .concat(
                    (HEALTH_PROBE_INTERVALS as readonly number[]).includes(
                      probe.intervalSecs,
                    )
                      ? []
                      : [probe.intervalSecs],
                  )
                  .map((secs) => (
                    <SelectItem key={secs} value={String(secs)}>
                      {secs < 60
                        ? t("settings.healthProbe.seconds", {
                            defaultValue: "{{count}} s",
                            count: secs,
                          })
                        : t("settings.healthProbe.minutes", {
                            defaultValue: "{{count}} min",
                            count: Math.round(secs / 60),
                          })}
                    </SelectItem>
                  ))}
              </SelectContent>
            </Select>
          </div>
        ) : null}
      </div>
    </section>
  );
}
