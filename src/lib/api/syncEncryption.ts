import { callJson } from "./invoke";
import { commands } from "@/lib/bindings";

/** Where the sync encryption passphrase is stored on this device. */
export type SyncSecretStorage = "keychain" | "settingsFile" | "none";

export interface SyncEncryptionStatus {
  enabled: boolean;
  hasPassphrase: boolean;
  storage: SyncSecretStorage;
  minPassphraseLength: number;
}

/** End-to-end encryption of WebDAV/S3 sync artifacts (SEC-06 / SVC-04). */
export const syncEncryptionApi = {
  async getStatus(): Promise<SyncEncryptionStatus> {
    return await callJson(commands.syncEncryptionGetStatus());
  },

  async setPassphrase(passphrase: string): Promise<SyncEncryptionStatus> {
    return await callJson(commands.syncEncryptionSetPassphrase(passphrase));
  },

  async disable(): Promise<SyncEncryptionStatus> {
    return await callJson(commands.syncEncryptionDisable());
  },
};
