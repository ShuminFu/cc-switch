import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import type { S3SyncSettings, WebDavSyncSettings } from "@/types";
import { CloudSyncPanel } from "./cloudSync/CloudSyncPanel";
import {
  SyncTypeSelector,
  useSyncTypeSelection,
} from "./cloudSync/SyncTypeSelector";
import { s3Adapter } from "./cloudSync/s3Adapter";
import { webdavAdapter } from "./cloudSync/webdavAdapter";
import { SyncEncryptionSettings } from "./SyncEncryptionSettings";

interface WebdavSyncSectionProps {
  config?: WebDavSyncSettings;
  s3Config?: S3SyncSettings;
  settings?: SettingsFormState;
  onAutoSave?: (updates: Partial<SettingsFormState>) => Promise<unknown>;
}

/**
 * Cloud sync settings: a backend selector plus the shared
 * {@link CloudSyncPanel} driven by the WebDAV or S3 adapter
 * (see `./cloudSync/`).
 */
export function WebdavSyncSection({
  config,
  s3Config,
  settings,
  onAutoSave,
}: WebdavSyncSectionProps) {
  const { t } = useTranslation();
  const selection = useSyncTypeSelection(config, s3Config);
  const [busy, setBusy] = useState(false);

  const confirmAutoSync = useCallback(
    () => onAutoSave?.({ autoSyncConfirmed: true }) ?? Promise.resolve(),
    [onAutoSave],
  );
  const panelOptions = {
    autoSyncConfirmed: settings?.autoSyncConfirmed,
    onConfirmAutoSync: confirmAutoSync,
    onBusyChange: setBusy,
  };

  return (
    <section className="space-y-4">
      <header className="space-y-2">
        <h3 className="text-base font-semibold text-foreground">
          {t("settings.webdavSync.title")}
        </h3>
        <p className="text-sm text-muted-foreground">
          {t("settings.webdavSync.description")}
        </p>
      </header>

      <SyncTypeSelector selection={selection} disabled={busy} />

      <SyncEncryptionSettings />

      {selection.syncType === "webdav" ? (
        <CloudSyncPanel
          key="webdav"
          adapter={webdavAdapter}
          config={config}
          {...panelOptions}
        />
      ) : (
        <CloudSyncPanel
          key="s3"
          adapter={s3Adapter}
          config={s3Config}
          {...panelOptions}
        />
      )}
    </section>
  );
}
