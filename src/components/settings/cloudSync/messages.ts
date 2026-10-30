import type { CloudSyncMessages } from "./types";

type DeepPartial<T> = { [K in keyof T]?: Partial<T[K]> | T[K] };

/**
 * The WebDAV and S3 locale sections share most key names, so a backend's
 * messages are derived from its prefix; `overrides` supplies the keys it
 * names differently or lacks.
 */
export function buildSyncMessages(
  prefix: string,
  overrides: DeepPartial<CloudSyncMessages> & { missingRequired: string },
): CloudSyncMessages {
  const k = (suffix: string) => `${prefix}.${suffix}`;
  const base: CloudSyncMessages = {
    missingRequired: overrides.missingRequired,
    presetsLabel: k("presets.label"),
    test: k("test"),
    testing: k("testing"),
    testSuccess: k("testSuccess"),
    testFailed: k("testFailed"),
    save: k("save"),
    saving: k("saving"),
    saveFailed: k("saveFailed"),
    saveAndTestSuccess: k("saveAndTestSuccess"),
    saveAndTestFailed: k("saveAndTestFailed"),
    unsaved: k("unsaved"),
    saved: k("saved"),
    unsavedChanges: k("unsavedChanges"),
    saveBeforeSync: k("saveBeforeSync"),
    upload: k("upload"),
    uploading: k("uploading"),
    uploadSuccess: k("uploadSuccess"),
    uploadFailed: k("uploadFailed"),
    download: k("download"),
    downloading: k("downloading"),
    downloadSuccess: k("downloadSuccess"),
    downloadFailed: k("downloadFailed"),
    fetchingRemote: k("fetchingRemote"),
    fetchRemoteFailed: k("fetchRemoteFailed"),
    noRemoteData: k("noRemoteData"),
    incompatibleVersion: k("incompatibleVersion"),
    lastSync: k("lastSync"),
    autoSync: k("autoSync"),
    autoSyncLastErrorTitle: k("autoSyncLastErrorTitle"),
    autoSyncLastErrorHint: k("autoSyncLastErrorHint"),
    confirmUpload: {
      title: k("confirmUpload.title"),
      content: k("confirmUpload.content"),
      dbItem: k("confirmUpload.dbItem"),
      skillsItem: k("confirmUpload.skillsItem"),
      targetPath: k("confirmUpload.targetPath"),
      existingData: k("confirmUpload.existingData"),
      deviceName: k("confirmUpload.deviceName"),
      createdAt: k("confirmUpload.createdAt"),
      path: k("confirmUpload.path"),
      dbCompat: k("confirmUpload.dbCompat"),
      warning: k("confirmUpload.warning"),
      legacyNotice: k("confirmUpload.legacyNotice"),
      confirm: k("confirmUpload.confirm"),
    },
    confirmDownload: {
      title: k("confirmDownload.title"),
      deviceName: k("confirmDownload.deviceName"),
      createdAt: k("confirmDownload.createdAt"),
      path: k("confirmDownload.path"),
      dbCompat: k("confirmDownload.dbCompat"),
      artifacts: k("confirmDownload.artifacts"),
      legacyNotice: k("confirmDownload.legacyNotice"),
      warning: k("confirmDownload.warning"),
      confirm: k("confirmDownload.confirm"),
    },
  };
  return {
    ...base,
    ...overrides,
    confirmUpload: { ...base.confirmUpload, ...overrides.confirmUpload },
    confirmDownload: { ...base.confirmDownload, ...overrides.confirmDownload },
  } as CloudSyncMessages;
}
