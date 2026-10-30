import type { HealthProbeConfig, NotificationToggles } from "@/types";

/** Mirrors `NotificationSettings::default()` in the backend. */
export const DEFAULT_NOTIFICATION_TOGGLES: NotificationToggles = {
  failover: true,
  circuitOpen: true,
  budgetExceeded: false,
  quotaExhausted: false,
  syncError: false,
};

/** Mirrors `HealthProbeSettings::default()` in the backend (opt-in). */
export const DEFAULT_HEALTH_PROBE: HealthProbeConfig = {
  enabled: false,
  intervalSecs: 120,
};

/** Probe interval choices, in seconds (backend clamps to 30–3600). */
export const HEALTH_PROBE_INTERVALS = [60, 120, 300, 600, 1800] as const;

export const NOTIFICATION_CATEGORIES = [
  "failover",
  "circuitOpen",
  "budgetExceeded",
  "quotaExhausted",
  "syncError",
] as const satisfies ReadonlyArray<keyof NotificationToggles>;

export function resolveNotificationToggles(
  value: Partial<NotificationToggles> | undefined | null,
): NotificationToggles {
  return { ...DEFAULT_NOTIFICATION_TOGGLES, ...(value ?? {}) };
}

export function resolveHealthProbe(
  value: Partial<HealthProbeConfig> | undefined | null,
): HealthProbeConfig {
  return { ...DEFAULT_HEALTH_PROBE, ...(value ?? {}) };
}
