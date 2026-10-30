import type { ProxyTakeoverStatus } from "@/types/proxy";
import { APP_REGISTRY, appsWith } from "./apps";

/**
 * Apps the local proxy can take over (and therefore configure failover for).
 * Mirrors the backend: `proxy_config.app_type` is limited to these four and
 * other apps are rejected by the proxy service. Every takeover/failover list
 * in the UI should be derived from this array.
 */
export const PROXY_CAPABLE_APPS = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
] as const;

export type ProxyCapableApp = (typeof PROXY_CAPABLE_APPS)[number];

export const isProxyCapableApp = (appId: string): appId is ProxyCapableApp =>
  (PROXY_CAPABLE_APPS as readonly string[]).includes(appId);

/** Display name for an app id, falling back to the raw id for unknown apps. */
export const getAppLabel = (appId: string): string =>
  (APP_REGISTRY as Record<string, { label: string } | undefined>)[appId]
    ?.label ?? appId;

/**
 * Registry-derived check that `PROXY_CAPABLE_APPS` (kept as a literal tuple
 * for its narrow type) agrees with the `proxy` capability in `APP_REGISTRY`.
 */
export const proxyCapableAppsMatchRegistry = (): boolean => {
  const fromRegistry = appsWith("proxy");
  return (
    fromRegistry.length === PROXY_CAPABLE_APPS.length &&
    fromRegistry.every((app, i) => app === PROXY_CAPABLE_APPS[i])
  );
};

/** True when the proxy has taken over any proxy-capable app. */
export const isAnyTakeoverActive = (
  status: Partial<ProxyTakeoverStatus> | null | undefined,
): boolean => PROXY_CAPABLE_APPS.some((app) => status?.[app] === true);
