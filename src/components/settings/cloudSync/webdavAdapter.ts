import { settingsApi } from "@/lib/api";
import type { WebDavSyncSettings } from "@/types";
import { buildSyncMessages } from "./messages";
import {
  CURRENT_REMOTE_LAYOUT_SEGMENT,
  DEFAULT_PROFILE,
  DEFAULT_REMOTE_ROOT,
  type CloudSyncBackendAdapter,
  type SyncPreset,
} from "./types";

export interface WebdavForm extends Record<string, string | boolean> {
  preset: string;
  baseUrl: string;
  username: string;
  password: string;
  remoteRoot: string;
  profile: string;
  autoSync: boolean;
}

interface WebdavPreset extends SyncPreset {
  baseUrl: string;
  matches?: (url: string) => boolean;
}

/** Port of the URL's authority, or null when it has none / cannot be parsed. */
function urlPort(url: string): string | null {
  try {
    return new URL(url.trim()).port || null;
  } catch {
    return null;
  }
}

export const WEBDAV_PRESETS: readonly WebdavPreset[] = [
  {
    id: "jianguoyun",
    label: "settings.webdavSync.presets.jianguoyun",
    baseUrl: "https://dav.jianguoyun.com/dav/",
    hint: "settings.webdavSync.presets.jianguoyunHint",
    matches: (url) => url.includes("jianguoyun.com"),
  },
  {
    id: "nextcloud",
    label: "settings.webdavSync.presets.nextcloud",
    baseUrl: "https://your-server/remote.php/dav/files/USERNAME/",
    hint: "settings.webdavSync.presets.nextcloudHint",
    matches: (url) => url.includes("remote.php/dav"),
  },
  {
    id: "synology",
    label: "settings.webdavSync.presets.synology",
    baseUrl: "http://your-nas-ip:5005/",
    hint: "settings.webdavSync.presets.synologyHint",
    // Synology's WebDAV Server package listens on 5005 (HTTP) / 5006 (HTTPS).
    // Match the port only, not ":5005" anywhere in the URL.
    matches: (url) => {
      const port = urlPort(url);
      return port === "5005" || port === "5006";
    },
  },
  {
    id: "custom",
    label: "settings.webdavSync.presets.custom",
    baseUrl: "",
  },
];

/** Match a URL to one of the preset providers, or "custom". */
export function detectWebdavPreset(url: string): string {
  if (!url) return "custom";
  return WEBDAV_PRESETS.find((preset) => preset.matches?.(url))?.id ?? "custom";
}

export const webdavAdapter: CloudSyncBackendAdapter<
  WebdavForm,
  WebDavSyncSettings
> = {
  id: "webdav",
  messages: buildSyncMessages("settings.webdavSync", {
    missingRequired: "settings.webdavSync.missingUrl",
  }),
  presets: WEBDAV_PRESETS,
  presetIsPersisted: true,
  secretField: "password",
  fields: [
    {
      name: "baseUrl",
      kind: "text",
      label: "settings.webdavSync.baseUrl",
      placeholder: (t) => t("settings.webdavSync.baseUrlPlaceholder"),
      // Editing the URL away from the selected preset switches to "custom".
      onBlur: (form) =>
        form.preset !== "custom" &&
        detectWebdavPreset(form.baseUrl) !== form.preset
          ? { preset: "custom" }
          : null,
    },
    {
      name: "username",
      kind: "text",
      label: "settings.webdavSync.username",
      placeholder: (t) => t("settings.webdavSync.usernamePlaceholder"),
    },
    {
      name: "password",
      kind: "password",
      label: "settings.webdavSync.password",
      placeholder: (t) => t("settings.webdavSync.passwordPlaceholder"),
    },
    "presetHint",
    {
      name: "remoteRoot",
      kind: "text",
      label: "settings.webdavSync.remoteRoot",
      hint: "settings.webdavSync.remoteRootDefault",
      placeholder: () => DEFAULT_REMOTE_ROOT,
    },
    {
      name: "profile",
      kind: "text",
      label: "settings.webdavSync.profile",
      hint: "settings.webdavSync.profileDefault",
      placeholder: () => DEFAULT_PROFILE,
    },
    {
      name: "autoSync",
      kind: "switch",
      label: "settings.webdavSync.autoSync",
      hint: "settings.webdavSync.autoSyncHint",
    },
  ],
  fromConfig: (config) => ({
    preset: detectWebdavPreset(config?.baseUrl ?? ""),
    baseUrl: config?.baseUrl ?? "",
    username: config?.username ?? "",
    password: config?.password ?? "",
    remoteRoot: config?.remoteRoot ?? DEFAULT_REMOTE_ROOT,
    profile: config?.profile ?? DEFAULT_PROFILE,
    autoSync: config?.autoSync ?? false,
  }),
  applyPreset: (form, presetId) => {
    const preset = WEBDAV_PRESETS.find((p) => p.id === presetId);
    if (!preset?.baseUrl)
      return { form: { ...form, preset: presetId }, changed: false };
    return {
      form: { ...form, preset: presetId, baseUrl: preset.baseUrl },
      changed: true,
    };
  },
  toSettings: (form, secretTouched) => {
    const baseUrl = form.baseUrl.trim();
    if (!baseUrl) return null;
    return {
      enabled: true,
      baseUrl,
      username: form.username.trim(),
      password: secretTouched ? form.password : "",
      remoteRoot: form.remoteRoot.trim() || DEFAULT_REMOTE_ROOT,
      profile: form.profile.trim() || DEFAULT_PROFILE,
      autoSync: form.autoSync,
    };
  },
  secretScopeKey: (values) =>
    JSON.stringify({
      baseUrl: (values.baseUrl ?? "").trim(),
      username: (values.username ?? "").trim(),
      remoteRoot: (values.remoteRoot ?? "").trim() || DEFAULT_REMOTE_ROOT,
      profile: (values.profile ?? "").trim() || DEFAULT_PROFILE,
    }),
  hasSavedConfig: (config) =>
    Boolean(config?.baseUrl?.trim() && config?.username?.trim()),
  buildRemotePath: (form) =>
    `/${form.remoteRoot.trim() || DEFAULT_REMOTE_ROOT}/${CURRENT_REMOTE_LAYOUT_SEGMENT}/${form.profile.trim() || DEFAULT_PROFILE}`,
  api: {
    test: (settings, preserveEmpty) =>
      settingsApi.webdavTestConnection(settings, preserveEmpty),
    save: (settings, secretTouched) =>
      settingsApi.webdavSyncSaveSettings(settings, secretTouched),
    fetchRemoteInfo: () => settingsApi.webdavSyncFetchRemoteInfo(),
    upload: (options) => settingsApi.webdavSyncUpload(options),
    download: (options) => settingsApi.webdavSyncDownload(options),
  },
};
