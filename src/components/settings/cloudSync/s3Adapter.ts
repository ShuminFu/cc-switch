import { settingsApi } from "@/lib/api";
import type { S3SyncSettings } from "@/types";
import { buildSyncMessages } from "./messages";
import {
  CURRENT_REMOTE_LAYOUT_SEGMENT,
  DEFAULT_PROFILE,
  DEFAULT_REMOTE_ROOT,
  type CloudSyncBackendAdapter,
  type SyncPreset,
} from "./types";

export interface S3Form extends Record<string, string | boolean> {
  preset: string;
  region: string;
  bucket: string;
  accessKeyId: string;
  secretAccessKey: string;
  endpoint: string;
  remoteRoot: string;
  profile: string;
  autoSync: boolean;
  enabled: boolean;
}

interface S3Preset extends SyncPreset {
  regionPlaceholder: string;
}

export const S3_PRESETS: readonly S3Preset[] = [
  {
    id: "aws-s3",
    label: "settings.s3Sync.presets.awsS3",
    hint: "settings.s3Sync.presets.awsS3Hint",
    regionPlaceholder: "us-east-1",
  },
  {
    id: "s3-minio",
    label: "settings.s3Sync.presets.minio",
    hint: "settings.s3Sync.presets.minioHint",
    regionPlaceholder: "us-east-1",
  },
  {
    id: "s3-r2",
    label: "settings.s3Sync.presets.r2",
    hint: "settings.s3Sync.presets.r2Hint",
    regionPlaceholder: "auto",
  },
  {
    id: "s3-oss",
    label: "settings.s3Sync.presets.oss",
    hint: "settings.s3Sync.presets.ossHint",
    regionPlaceholder: "cn-hangzhou",
  },
  {
    id: "s3-cos",
    label: "settings.s3Sync.presets.cos",
    hint: "settings.s3Sync.presets.cosHint",
    regionPlaceholder: "ap-guangzhou",
  },
  {
    id: "s3-obs",
    label: "settings.s3Sync.presets.obs",
    hint: "settings.s3Sync.presets.obsHint",
    regionPlaceholder: "cn-north-4",
  },
  {
    id: "s3-custom",
    label: "settings.s3Sync.presets.custom",
    hint: "settings.s3Sync.presets.customHint",
    regionPlaceholder: "us-east-1",
  },
];

export const s3Adapter: CloudSyncBackendAdapter<S3Form, S3SyncSettings> = {
  id: "s3",
  messages: buildSyncMessages("settings.s3Sync", {
    missingRequired: "settings.s3Sync.missingBucket",
    // Keys the S3 locale section does not define; the WebDAV wording is
    // backend-neutral.
    confirmUpload: {
      path: "settings.webdavSync.confirmUpload.path",
      dbCompat: "settings.webdavSync.confirmUpload.dbCompat",
      legacyNotice: "settings.webdavSync.confirmUpload.legacyNotice",
    },
    confirmDownload: {
      path: "settings.webdavSync.confirmDownload.path",
      dbCompat: "settings.webdavSync.confirmDownload.dbCompat",
      legacyNotice: "settings.webdavSync.confirmDownload.legacyNotice",
    },
  }),
  presets: S3_PRESETS,
  presetIsPersisted: false,
  secretField: "secretAccessKey",
  fields: [
    "presetHint",
    {
      name: "region",
      kind: "text",
      label: "settings.s3Sync.region",
      placeholder: (_t, form) =>
        S3_PRESETS.find((p) => p.id === form.preset)?.regionPlaceholder ??
        "us-east-1",
    },
    {
      name: "bucket",
      kind: "text",
      label: "settings.s3Sync.bucket",
      placeholder: (t) => t("settings.s3Sync.bucketPlaceholder"),
    },
    {
      name: "accessKeyId",
      kind: "text",
      label: "settings.s3Sync.accessKeyId",
      placeholder: (t) => t("settings.s3Sync.accessKeyIdPlaceholder"),
    },
    {
      name: "secretAccessKey",
      kind: "password",
      label: "settings.s3Sync.secretAccessKey",
      placeholder: (t) => t("settings.s3Sync.secretAccessKeyPlaceholder"),
    },
    {
      name: "endpoint",
      kind: "text",
      label: "settings.s3Sync.endpoint",
      hint: "settings.s3Sync.endpointHint",
      placeholder: (t) => t("settings.s3Sync.endpointPlaceholder"),
    },
    {
      name: "remoteRoot",
      kind: "text",
      label: "settings.s3Sync.remoteRoot",
      hint: "settings.s3Sync.remoteRootDefault",
      placeholder: () => DEFAULT_REMOTE_ROOT,
    },
    {
      name: "profile",
      kind: "text",
      label: "settings.s3Sync.profile",
      hint: "settings.s3Sync.profileDefault",
      placeholder: () => DEFAULT_PROFILE,
    },
    {
      name: "autoSync",
      kind: "switch",
      label: "settings.s3Sync.autoSync",
      hint: "settings.s3Sync.autoSyncHint",
    },
    {
      name: "enabled",
      kind: "switch",
      label: "settings.s3Sync.enabled",
      hint: "settings.s3Sync.enabledHint",
    },
  ],
  fromConfig: (config) => ({
    // The preset only picks hints/placeholders and is not persisted.
    preset: "aws-s3",
    region: config?.region ?? "",
    bucket: config?.bucket ?? "",
    accessKeyId: config?.accessKeyId ?? "",
    secretAccessKey: config?.secretAccessKey ?? "",
    endpoint: config?.endpoint ?? "",
    remoteRoot: config?.remoteRoot ?? DEFAULT_REMOTE_ROOT,
    profile: config?.profile ?? DEFAULT_PROFILE,
    autoSync: config?.autoSync ?? false,
    enabled: config?.enabled ?? false,
  }),
  applyPreset: (form, presetId) => ({
    form: { ...form, preset: presetId },
    changed: true,
  }),
  toSettings: (form, secretTouched) => {
    const bucket = form.bucket.trim();
    if (!bucket) return null;
    return {
      enabled: form.enabled,
      autoSync: form.autoSync,
      region: form.region.trim(),
      bucket,
      accessKeyId: form.accessKeyId.trim(),
      secretAccessKey: secretTouched ? form.secretAccessKey : "",
      endpoint: form.endpoint.trim() || undefined,
      remoteRoot: form.remoteRoot.trim() || DEFAULT_REMOTE_ROOT,
      profile: form.profile.trim() || DEFAULT_PROFILE,
    };
  },
  secretScopeKey: (values) =>
    JSON.stringify({
      endpoint: (values.endpoint ?? "").trim(),
      region: (values.region ?? "").trim(),
      bucket: (values.bucket ?? "").trim(),
      accessKeyId: (values.accessKeyId ?? "").trim(),
    }),
  hasSavedConfig: (config) =>
    Boolean(config?.bucket?.trim() && config?.accessKeyId?.trim()),
  buildRemotePath: (form) =>
    `${form.bucket.trim() || "bucket"}/${form.remoteRoot.trim() || DEFAULT_REMOTE_ROOT}/${CURRENT_REMOTE_LAYOUT_SEGMENT}/${form.profile.trim() || DEFAULT_PROFILE}`,
  api: {
    test: (settings, preserveEmpty) =>
      settingsApi.s3TestConnection(settings, preserveEmpty),
    save: (settings, secretTouched) =>
      settingsApi.s3SyncSaveSettings(settings, secretTouched),
    fetchRemoteInfo: () => settingsApi.s3SyncFetchRemoteInfo(),
    upload: (options) => settingsApi.s3SyncUpload(options),
    download: (options) => settingsApi.s3SyncDownload(options),
  },
};
