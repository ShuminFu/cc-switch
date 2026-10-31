//! Commands for end-to-end sync encryption (SEC-06 / SVC-04).
//!
//! The passphrase never goes back to the webview: the status command only
//! reports whether one is set and where it is stored.

use serde_json::{json, Value};

use crate::secrets::envelope::MIN_PASSPHRASE_CHARS;
use crate::secrets::sync_credentials::{self, SYNC_PASSPHRASE_ACCOUNT};
use crate::settings::{self, SyncEncryptionSettings};

fn status_json(current: Option<&SyncEncryptionSettings>) -> Value {
    let passphrase = current.map(|s| s.passphrase.as_str()).unwrap_or_default();
    json!({
        "enabled": current.is_some_and(|s| s.enabled),
        "hasPassphrase": !passphrase.is_empty(),
        // "keychain" | "settingsFile" | "none"
        "storage": sync_credentials::location(SYNC_PASSPHRASE_ACCOUNT, passphrase).as_str(),
        "minPassphraseLength": MIN_PASSPHRASE_CHARS,
    })
}

fn validate_passphrase(passphrase: &str) -> Result<(), String> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(crate::error::AppError::localized(
            "sync.encryption.passphrase_too_short",
            format!("加密密码至少需要 {MIN_PASSPHRASE_CHARS} 个字符"),
            format!(
                "The encryption passphrase must be at least {MIN_PASSPHRASE_CHARS} characters."
            ),
        )
        .to_string());
    }
    Ok(())
}

/// Encryption status (never includes the passphrase).
#[tauri::command]
#[specta::specta]
pub async fn sync_encryption_get_status() -> Result<Value, String> {
    Ok(status_json(
        settings::get_sync_encryption_settings().as_ref(),
    ))
}

/// Enable encryption with `passphrase` (replacing any previous one).
///
/// The next upload is encrypted with the new passphrase; other devices need
/// the same passphrase to download it.
#[tauri::command]
#[specta::specta]
pub async fn sync_encryption_set_passphrase(passphrase: String) -> Result<Value, String> {
    validate_passphrase(&passphrase)?;
    let next = SyncEncryptionSettings {
        enabled: true,
        passphrase,
        passphrase_in_keychain: false,
    };
    settings::set_sync_encryption_settings(Some(next)).map_err(|e| e.to_string())?;
    Ok(status_json(
        settings::get_sync_encryption_settings().as_ref(),
    ))
}

/// Turn encryption off and forget the passphrase (also from the keychain).
#[tauri::command]
#[specta::specta]
pub async fn sync_encryption_disable() -> Result<Value, String> {
    settings::set_sync_encryption_settings(None).map_err(|e| e.to_string())?;
    Ok(status_json(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_passphrase_is_rejected() {
        assert!(validate_passphrase("short").is_err());
        assert!(validate_passphrase("long enough pass").is_ok());
        // Counted in characters, not bytes.
        assert!(validate_passphrase("密码密码密码密码").is_ok());
    }

    #[test]
    fn status_never_contains_the_passphrase() {
        let s = SyncEncryptionSettings {
            enabled: true,
            passphrase: "top secret phrase".to_string(),
            passphrase_in_keychain: false,
        };
        let json = status_json(Some(&s)).to_string();
        assert!(!json.contains("top secret phrase"));
        assert!(json.contains("\"hasPassphrase\":true"));
    }
}
