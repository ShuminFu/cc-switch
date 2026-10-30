import { invoke } from "@tauri-apps/api/core";

/** State of the provider API-key vault on this device (DB-02 / PRODUCT-01). */
export interface SecretsVaultStatus {
  enabled: boolean;
  /** "keychain" | "memory" | "none" */
  backend: string;
  /** Whether the system keychain can store values right now. */
  available: boolean;
  unavailableReason: string | null;
  /** Secrets stored as placeholders (values in the keychain). */
  vaultedSecrets: number;
  /** Secrets still stored in plain text in the database. */
  inlineSecrets: number;
  /** Placeholders whose key is missing from this device's keychain. */
  unresolvedSecrets: number;
  /** `<app>/<provider id>` of providers whose keys must be re-entered. */
  unresolvedProviders: string[];
  /** Problem reported by the last switch, if any. */
  warning: string | null;
}

export const secretsVaultApi = {
  async getStatus(): Promise<SecretsVaultStatus> {
    return await invoke("get_provider_secret_vault_status");
  },

  async setEnabled(enabled: boolean): Promise<SecretsVaultStatus> {
    return await invoke("set_provider_secret_vault", { enabled });
  },
};
