//! Switching the provider API-key vault on and off (DB-02 / PRODUCT-01).
//!
//! The flag (`AppSettings::provider_secret_vault`) is changed only here,
//! together with the migration of the stored rows:
//! - **on**: refuses when no keychain works; takes a safety backup; then moves
//!   inline secrets of every provider (and universal provider) into the
//!   keychain and stores placeholders.
//! - **off**: takes a safety backup; inlines every placeholder that resolves,
//!   persists the rows, and only then deletes the keychain entries. Entries
//!   that cannot be read are reported; their keys must be re-entered.
//!
//! Both directions can be re-run: a partial migration leaves a valid mixed
//! state (placeholders and inline values coexist) that the status reports.

use serde::Serialize;

use super::provider_vault::{self, ProviderRow};
use super::SecretError;
use crate::database::Database;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    /// The setting (device-local, default off).
    pub enabled: bool,
    /// `keychain`, `memory` or `none`.
    pub backend: String,
    /// Whether the keychain can store values right now.
    pub available: bool,
    pub unavailable_reason: Option<String>,
    /// Secrets currently stored as placeholders.
    pub vaulted_secrets: usize,
    /// Secrets still stored inline in the database.
    pub inline_secrets: usize,
    /// Placeholders whose keychain entry is missing on this device.
    pub unresolved_secrets: usize,
    /// `<app>/<provider id>` of the providers with unresolved secrets.
    pub unresolved_providers: Vec<String>,
    /// Problem reported by the last toggle, if any.
    pub warning: Option<String>,
}

fn all_rows(db: &Database) -> Result<Vec<ProviderRow>, AppError> {
    let mut rows = db.load_provider_secret_rows()?;
    rows.extend(db.load_universal_secret_rows()?);
    Ok(rows)
}

/// Current state of the vault on this device.
pub fn status(db: &Database) -> Result<VaultStatus, AppError> {
    let backend = provider_vault::vault_backend();
    let probe = provider_vault::check_available(backend);
    let mut status = VaultStatus {
        enabled: provider_vault::enabled(),
        backend: backend.name().to_string(),
        available: probe.is_ok(),
        unavailable_reason: probe.err().map(|e| e.to_string()),
        vaulted_secrets: 0,
        inline_secrets: 0,
        unresolved_secrets: 0,
        unresolved_providers: Vec::new(),
        warning: None,
    };
    for row in all_rows(db)? {
        status.inline_secrets += provider_vault::inline_secret_count(&row.settings_config);
        let refs = provider_vault::placeholder_refs(&row.settings_config);
        status.vaulted_secrets += refs.len();
        let missing = refs
            .iter()
            .filter(|r| !provider_vault::ref_exists(backend, r))
            .count();
        if missing > 0 {
            status.unresolved_secrets += missing;
            status
                .unresolved_providers
                .push(format!("{}/{}", row.app, row.id));
        }
    }
    Ok(status)
}

fn keychain_unavailable(err: &SecretError) -> AppError {
    AppError::localized(
        "secrets.vault.keychain_unavailable",
        format!(
            "没有可用的系统钥匙串，无法把供应商 API 密钥移出数据库。Linux 上请在桌面会话中启动 Secret Service（GNOME Keyring 或 KWallet）后重试。（{err}）"
        ),
        format!(
            "No system keychain is available, so provider API keys cannot be moved out of the database. On Linux, run a Secret Service provider (GNOME Keyring or KWallet) in your desktop session and try again. ({err})"
        ),
    )
}

/// Persist only the rows whose config changed.
fn store_changed(
    db: &Database,
    before: &[ProviderRow],
    after: &[ProviderRow],
) -> Result<(), AppError> {
    let (mut providers, mut universal) = (Vec::new(), Vec::new());
    for (old, new) in before.iter().zip(after) {
        if old.settings_config == new.settings_config {
            continue;
        }
        let row = ProviderRow {
            app: new.app.clone(),
            id: new.id.clone(),
            settings_config: new.settings_config.clone(),
        };
        if new.app == provider_vault::UNIVERSAL_APP {
            universal.push(row);
        } else {
            providers.push(row);
        }
    }
    db.store_provider_secret_rows(&providers)?;
    db.store_universal_secret_rows(&universal)?;
    Ok(())
}

fn snapshot(rows: &[ProviderRow]) -> Vec<ProviderRow> {
    rows.iter()
        .map(|r| ProviderRow {
            app: r.app.clone(),
            id: r.id.clone(),
            settings_config: r.settings_config.clone(),
        })
        .collect()
}

/// Switch the vault on or off and migrate the stored rows.
pub fn set_enabled(db: &Database, enabled: bool) -> Result<VaultStatus, AppError> {
    let backend = provider_vault::vault_backend();
    if enabled {
        provider_vault::check_available(backend).map_err(|e| keychain_unavailable(&e))?;
    }

    safety_backup(db)?;
    set_flag(enabled)?;

    let mut rows = all_rows(db)?;
    let before = snapshot(&rows);
    let warning = if enabled {
        let (done, err) = provider_vault::migrate_rows_to_vault(backend, &mut rows);
        store_changed(db, &before[..done], &rows[..done])?;
        err.map(|e| {
            log::warn!("[Vault] Migration into the keychain stopped after {done} row(s): {e}");
            format!("{e}")
        })
    } else {
        let outcome = provider_vault::migrate_rows_from_vault(backend, &mut rows);
        store_changed(db, &before, &rows)?;
        // Rows are persisted with inline values: now the entries can go.
        provider_vault::delete_refs(backend, &outcome.restored);
        if outcome.unresolved.is_empty() {
            None
        } else {
            log::warn!(
                "[Vault] {} secret(s) could not be read from the keychain and stay as placeholders",
                outcome.unresolved.len()
            );
            Some(format!(
                "{} secret(s) could not be read from the keychain; re-enter those API keys",
                outcome.unresolved.len()
            ))
        }
    };

    let mut status = status(db)?;
    status.warning = warning;
    Ok(status)
}

/// Safety backup of the database before provider rows are rewritten.
#[cfg(not(test))]
fn safety_backup(db: &Database) -> Result<(), AppError> {
    db.backup_database_file_of_kind(crate::database::backup::BackupKind::Safety, None)?;
    Ok(())
}

/// Unit tests use in-memory databases; never write into the user config dir.
#[cfg(test)]
fn safety_backup(_db: &Database) -> Result<(), AppError> {
    Ok(())
}

#[cfg(not(test))]
fn set_flag(enabled: bool) -> Result<(), AppError> {
    crate::settings::set_provider_secret_vault(enabled)
}

/// Tests never write the shared settings file: the flag is thread-local.
#[cfg(test)]
fn set_flag(enabled: bool) -> Result<(), AppError> {
    provider_vault::set_enabled_for_test(Some(enabled));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use serde_json::json;

    fn provider(id: &str, key: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            id.to_string(),
            json!({ "env": { "ANTHROPIC_AUTH_TOKEN": key, "ANTHROPIC_BASE_URL": "https://x" } }),
            None,
        )
    }

    fn raw_config(db: &Database, id: &str) -> serde_json::Value {
        db.load_provider_secret_rows()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap()
            .settings_config
    }

    #[test]
    fn toggle_migrates_rows_both_ways_and_dao_keeps_returning_real_values() {
        let db = Database::memory().unwrap();
        provider_vault::set_enabled_for_test(Some(false));
        db.save_provider("claude", &provider("vault-toggle-a", "sk-toggle-a"))
            .unwrap();
        db.save_provider("claude", &provider("vault-toggle-b", "sk-toggle-b"))
            .unwrap();
        assert!(raw_config(&db, "vault-toggle-a")
            .to_string()
            .contains("sk-toggle-a"));

        let status = set_enabled(&db, true).unwrap();
        assert!(status.enabled);
        assert_eq!(status.inline_secrets, 0);
        assert!(status.vaulted_secrets >= 2);
        let raw = raw_config(&db, "vault-toggle-a").to_string();
        assert!(!raw.contains("sk-toggle-a"), "{raw}");
        assert!(raw.contains(provider_vault::PLACEHOLDER_PREFIX));
        // Every reader still sees the real key.
        let p = db
            .get_provider_by_id("vault-toggle-a", "claude")
            .unwrap()
            .unwrap();
        assert_eq!(
            p.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
            json!("sk-toggle-a")
        );
        let all = db.get_all_providers("claude").unwrap();
        assert_eq!(
            all["vault-toggle-b"].settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
            json!("sk-toggle-b")
        );

        // New writes while on are vaulted too.
        db.save_provider("claude", &provider("vault-toggle-a", "sk-toggle-a2"))
            .unwrap();
        assert!(!raw_config(&db, "vault-toggle-a")
            .to_string()
            .contains("sk-toggle-a2"));

        let status = set_enabled(&db, false).unwrap();
        assert!(!status.enabled);
        assert_eq!(status.vaulted_secrets, 0);
        assert!(status.warning.is_none());
        assert!(raw_config(&db, "vault-toggle-a")
            .to_string()
            .contains("sk-toggle-a2"));
        assert!(!provider_vault::ref_exists(
            provider_vault::vault_backend(),
            "claude/vault-toggle-a/env/ANTHROPIC_AUTH_TOKEN"
        ));
        provider_vault::set_enabled_for_test(None);
    }

    #[test]
    fn deleting_a_provider_removes_its_keychain_entries() {
        let db = Database::memory().unwrap();
        provider_vault::set_enabled_for_test(Some(true));
        db.save_provider("claude", &provider("vault-delete", "sk-delete"))
            .unwrap();
        let reference = "claude/vault-delete/env/ANTHROPIC_AUTH_TOKEN";
        assert!(provider_vault::ref_exists(
            provider_vault::vault_backend(),
            reference
        ));
        db.delete_provider("claude", "vault-delete").unwrap();
        assert!(!provider_vault::ref_exists(
            provider_vault::vault_backend(),
            reference
        ));
        provider_vault::set_enabled_for_test(None);
    }

    #[test]
    fn universal_provider_keys_are_vaulted() {
        let db = Database::memory().unwrap();
        provider_vault::set_enabled_for_test(Some(true));
        let mut up = crate::provider::UniversalProvider::new(
            "vault-universal".to_string(),
            "U".to_string(),
            "custom".to_string(),
            "https://u".to_string(),
            "sk-universal".to_string(),
        );
        up.notes = Some("n".to_string());
        db.save_universal_provider(&up).unwrap();
        let raw = db.load_universal_secret_rows().unwrap();
        assert!(!raw[0].settings_config.to_string().contains("sk-universal"));
        assert_eq!(
            db.get_universal_provider("vault-universal")
                .unwrap()
                .unwrap()
                .api_key,
            "sk-universal"
        );
        db.delete_universal_provider("vault-universal").unwrap();
        assert!(!provider_vault::ref_exists(
            provider_vault::vault_backend(),
            "universal/vault-universal/apiKey"
        ));
        provider_vault::set_enabled_for_test(None);
    }
}
