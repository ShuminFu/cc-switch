import type { TFunction } from "i18next";
import type {
  CloudRemoteInfoResult,
  CloudSyncTransferOptions,
  WebDavSyncResult,
  WebDavTestResult,
} from "@/lib/api/settings";
import type { WebDavSyncStatus } from "@/types";

export type SyncBackendId = "webdav" | "s3";

export type ActionState =
  | "idle"
  | "testing"
  | "saving"
  | "uploading"
  | "downloading"
  | "fetching_remote";

/** Values of a backend's form. Every form carries the selected preset id. */
export type SyncFormValues = Record<string, string | boolean> & {
  preset: string;
  autoSync: boolean;
};

export interface SyncPreset {
  id: string;
  /** i18n key. */
  label: string;
  /** i18n key, shown under the preset selector's fields. */
  hint?: string;
}

/** One input of the backend form; the panel renders the list in order. */
export interface SyncFieldSpec<F extends SyncFormValues> {
  name: Extract<keyof F, string>;
  kind: "text" | "password" | "switch";
  /** i18n key. */
  label: string;
  /** i18n key shown in small print under the label. */
  hint?: string;
  placeholder?: (t: TFunction, form: F) => string;
  /** Patch applied to the form when the input loses focus. */
  onBlur?: (form: F) => Partial<F> | null;
}

/** `"presetHint"` marks where the active preset's hint is rendered. */
export type SyncFieldEntry<F extends SyncFormValues> =
  | SyncFieldSpec<F>
  | "presetHint";

/** i18n keys a backend supplies for the shared panel, dialogs and toasts. */
export interface CloudSyncMessages {
  missingRequired: string;
  presetsLabel: string;
  test: string;
  testing: string;
  testSuccess: string;
  testFailed: string;
  save: string;
  saving: string;
  saveFailed: string;
  saveAndTestSuccess: string;
  saveAndTestFailed: string;
  unsaved: string;
  saved: string;
  unsavedChanges: string;
  saveBeforeSync: string;
  upload: string;
  uploading: string;
  uploadSuccess: string;
  uploadFailed: string;
  download: string;
  downloading: string;
  downloadSuccess: string;
  downloadFailed: string;
  fetchingRemote: string;
  fetchRemoteFailed: string;
  noRemoteData: string;
  incompatibleVersion: string;
  lastSync: string;
  autoSync: string;
  autoSyncLastErrorTitle: string;
  autoSyncLastErrorHint: string;
  confirmUpload: {
    title: string;
    content: string;
    dbItem: string;
    skillsItem: string;
    targetPath: string;
    existingData: string;
    deviceName: string;
    createdAt: string;
    path: string;
    dbCompat: string;
    warning: string;
    legacyNotice: string;
    confirm: string;
  };
  confirmDownload: {
    title: string;
    deviceName: string;
    createdAt: string;
    path: string;
    dbCompat: string;
    artifacts: string;
    legacyNotice: string;
    warning: string;
    confirm: string;
  };
}

export interface CloudSyncApi<C> {
  test: (
    settings: C,
    preserveEmptySecret: boolean,
  ) => Promise<WebDavTestResult>;
  save: (settings: C, secretTouched: boolean) => Promise<{ success: boolean }>;
  fetchRemoteInfo: () => Promise<CloudRemoteInfoResult>;
  upload: (options: CloudSyncTransferOptions) => Promise<WebDavSyncResult>;
  download: (options: CloudSyncTransferOptions) => Promise<WebDavSyncResult>;
}

/**
 * Everything backend-specific about a cloud sync target. The panel, form
 * state machine and dialogs are shared; adding a backend means adding an
 * adapter.
 */
export interface CloudSyncBackendAdapter<
  F extends SyncFormValues,
  C extends { status?: WebDavSyncStatus },
> {
  id: SyncBackendId;
  messages: CloudSyncMessages;
  presets: readonly SyncPreset[];
  /** `fromConfig` derives the preset from the config (else it is UI-only). */
  presetIsPersisted: boolean;
  fields: readonly SyncFieldEntry<F>[];
  /** The credential the backend keeps when an empty value is submitted. */
  secretField: Extract<keyof F, string>;
  /** Form values for a (possibly missing) persisted config. */
  fromConfig: (config: C | undefined) => F;
  /** Select a preset; `changed` means the form content changed (dirty). */
  applyPreset: (form: F, presetId: string) => { form: F; changed: boolean };
  /**
   * Settings to submit, or `null` when a required field is missing. An
   * untouched secret is submitted empty so the backend keeps the stored one.
   */
  toSettings: (form: F, secretTouched: boolean) => C | null;
  /**
   * Identity of the remote a secret belongs to. After a save, the redacted
   * config refetch keeps the typed secret only while this stays the same.
   */
  secretScopeKey: (values: Partial<F>) => string;
  hasSavedConfig: (config: C | undefined) => boolean;
  /** Display path the next upload writes to (used before the remote reports one). */
  buildRemotePath: (form: F) => string;
  api: CloudSyncApi<C>;
}

/** Remote layout segment the current client writes (`v2/db-v6`). */
export const CURRENT_REMOTE_LAYOUT_SEGMENT = "v2/db-v6";

export const DEFAULT_REMOTE_ROOT = "cc-switch-sync";
export const DEFAULT_PROFILE = "default";
