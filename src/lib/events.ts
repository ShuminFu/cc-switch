import type { AppId } from "@/lib/api/types";
import type { UsageResult } from "@/types";
import type { SubscriptionQuota } from "@/types/subscription";

/**
 * The backend -> frontend event contract (FE-ARCH-19). Every name the Rust
 * side emits (`app.emit("...")` or an `*_EVENT` constant) is a key here, with
 * its payload type; `useTauriEvent` only accepts these keys, so renaming an
 * event on either side fails the build or `tests/lib/appEvents.test.ts`.
 */

export interface ProviderSwitchedPayload {
  appType: AppId;
  providerId: string;
}

export type UsageCacheUpdatedPayload =
  | {
      kind: "script";
      appType: AppId;
      providerId: string;
      data: UsageResult;
    }
  | {
      kind: "subscription";
      appType: AppId;
      data: SubscriptionQuota;
    };

export interface SyncStatusUpdatedPayload {
  source?: string;
  status?: string;
  error?: string;
}

export interface ProxyOfficialWarningPayload {
  appType: string;
  providerName: string;
}

export interface UniversalProviderSyncedPayload {
  action: string;
  id: string;
}

export interface UpdateDownloadProgressPayload {
  downloaded: number;
  total: number | null;
}

export interface DeeplinkErrorPayload {
  url: string;
  error: string;
}

export interface AppEvents {
  "provider-switched": ProviderSwitchedPayload;
  "usage-cache-updated": UsageCacheUpdatedPayload;
  "usage-log-recorded": unknown;
  "proxy-flags-changed": unknown;
  "proxy-official-warning": ProxyOfficialWarningPayload;
  "universal-provider-synced": UniversalProviderSyncedPayload;
  "profile-applied": unknown;
  "webdav-sync-status-updated": SyncStatusUpdatedPayload | null | undefined;
  "s3-sync-status-updated": SyncStatusUpdatedPayload | null | undefined;
  /** Payload is a `DeepLinkImportRequestV2` (see `@/lib/api/deeplink`). */
  "deeplink-import": unknown;
  "deeplink-error": DeeplinkErrorPayload;
  /** A navigation/switch action link is waiting in the backend mailbox. */
  "deeplink-action": unknown;
  "update-download-progress": UpdateDownloadProgressPayload;
  "startup-progress": unknown;
  "startup-ready": unknown;
}

export type AppEventName = keyof AppEvents;

/** Runtime list of the contract's names (kept in sync by the type below). */
export const APP_EVENT_NAMES = [
  "provider-switched",
  "usage-cache-updated",
  "usage-log-recorded",
  "proxy-flags-changed",
  "proxy-official-warning",
  "universal-provider-synced",
  "profile-applied",
  "webdav-sync-status-updated",
  "s3-sync-status-updated",
  "deeplink-import",
  "deeplink-error",
  "deeplink-action",
  "update-download-progress",
  "startup-progress",
  "startup-ready",
] as const satisfies readonly AppEventName[];

// Compile-time check that APP_EVENT_NAMES lists every key of AppEvents.
type MissingEventNames = Exclude<
  AppEventName,
  (typeof APP_EVENT_NAMES)[number]
>;
const _allEventNamesListed: MissingEventNames extends never ? true : never =
  true;
void _allEventNamesListed;
