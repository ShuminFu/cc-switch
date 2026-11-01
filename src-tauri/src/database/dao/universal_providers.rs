//! 统一供应商 (Universal Provider) DAO
//!
//! 提供统一供应商的 CRUD 操作。
//!
//! DB-02: with the provider API-key vault on, each provider's `apiKey` is kept
//! in the OS keychain (reference `universal/<id>/apiKey`) and the stored JSON
//! holds a placeholder. Reads always return the real value.

use crate::database::{lock_conn, to_json_string, Database};
use crate::error::AppError;
use crate::provider::UniversalProvider;
use crate::secrets::provider_vault::{self, ProviderRow};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// 统一供应商的 Settings Key
const UNIVERSAL_PROVIDERS_KEY: &str = "universal_providers";

use provider_vault::UNIVERSAL_APP as UNIVERSAL_VAULT_APP;

impl Database {
    /// The stored JSON object (placeholders unresolved).
    fn load_raw_universal_providers(&self) -> Result<Map<String, Value>, AppError> {
        let conn = lock_conn!(self.conn);
        let result: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?",
                [UNIVERSAL_PROVIDERS_KEY],
                |row| row.get(0),
            )
            .ok();
        match result {
            Some(json) => match serde_json::from_str::<Value>(&json)
                .map_err(|e| AppError::Database(format!("解析统一供应商数据失败: {e}")))?
            {
                Value::Object(map) => Ok(map),
                _ => Err(AppError::Database(
                    "解析统一供应商数据失败: not an object".to_string(),
                )),
            },
            None => Ok(Map::new()),
        }
    }

    fn store_raw_universal_providers(&self, map: &Map<String, Value>) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let json = to_json_string(map)?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
            [UNIVERSAL_PROVIDERS_KEY, &json],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 获取所有统一供应商
    pub fn get_all_universal_providers(
        &self,
    ) -> Result<HashMap<String, UniversalProvider>, AppError> {
        let raw = self.load_raw_universal_providers()?;
        let mut out = HashMap::with_capacity(raw.len());
        for (id, value) in raw {
            let value = provider_vault::for_read(UNIVERSAL_VAULT_APP, &id, value);
            let provider: UniversalProvider = serde_json::from_value(value)
                .map_err(|e| AppError::Database(format!("解析统一供应商数据失败: {e}")))?;
            out.insert(id, provider);
        }
        Ok(out)
    }

    /// 获取单个统一供应商
    pub fn get_universal_provider(&self, id: &str) -> Result<Option<UniversalProvider>, AppError> {
        let providers = self.get_all_universal_providers()?;
        Ok(providers.get(id).cloned())
    }

    fn lock_universal_providers(&self) -> Result<std::sync::MutexGuard<'_, ()>, AppError> {
        self.universal_providers_lock
            .lock()
            .map_err(|e| AppError::Lock(e.to_string()))
    }

    /// 保存统一供应商（添加或更新）
    ///
    /// DB-16: the read-modify-write of the stored map is serialised, and only
    /// this provider's entry is re-encoded, so concurrent saves of different
    /// ids no longer lose one another.
    pub fn save_universal_provider(&self, provider: &UniversalProvider) -> Result<(), AppError> {
        let _guard = self.lock_universal_providers()?;
        let mut raw = self.load_raw_universal_providers()?;
        let value = serde_json::to_value(provider)
            .map_err(|e| AppError::Database(format!("序列化统一供应商失败: {e}")))?;
        let stored = provider_vault::for_storage(UNIVERSAL_VAULT_APP, &provider.id, &value);
        let previous = raw.insert(provider.id.clone(), stored.clone());
        self.store_raw_universal_providers(&raw)?;
        if let Some(old) = previous {
            provider_vault::forget_stale(UNIVERSAL_VAULT_APP, &provider.id, &old, Some(&stored));
        }
        Ok(())
    }

    /// 删除统一供应商
    pub fn delete_universal_provider(&self, id: &str) -> Result<bool, AppError> {
        let _guard = self.lock_universal_providers()?;
        let mut raw = self.load_raw_universal_providers()?;
        let Some(old) = raw.remove(id) else {
            return Ok(false);
        };
        self.store_raw_universal_providers(&raw)?;
        provider_vault::forget_stale(UNIVERSAL_VAULT_APP, id, &old, None);
        Ok(true)
    }

    /// Stored universal providers as vault rows (app `universal`), DB-02.
    pub fn load_universal_secret_rows(&self) -> Result<Vec<ProviderRow>, AppError> {
        Ok(self
            .load_raw_universal_providers()?
            .into_iter()
            .map(|(id, settings_config)| ProviderRow {
                app: UNIVERSAL_VAULT_APP.to_string(),
                id,
                settings_config,
            })
            .collect())
    }

    /// Write back the given universal provider rows as-is (no vault processing).
    pub fn store_universal_secret_rows(&self, rows: &[ProviderRow]) -> Result<(), AppError> {
        if rows.is_empty() {
            return Ok(());
        }
        let _guard = self.lock_universal_providers()?;
        let mut map = self.load_raw_universal_providers()?;
        for row in rows {
            if map.contains_key(&row.id) {
                map.insert(row.id.clone(), row.settings_config.clone());
            }
        }
        self.store_raw_universal_providers(&map)
    }
}

#[cfg(test)]
mod tests {
    use crate::database::Database;
    use crate::provider::UniversalProvider;

    /// DB-16: concurrent saves of different ids keep both.
    #[test]
    fn concurrent_saves_of_different_ids_keep_both() {
        let db = Database::memory().expect("db");
        std::thread::scope(|scope| {
            for t in 0..2 {
                let db = &db;
                scope.spawn(move || {
                    for i in 0..25 {
                        let mut p: UniversalProvider = serde_json::from_value(serde_json::json!({
                            "id": format!("t{t}-{i}"),
                            "name": format!("t{t}-{i}"),
                            "providerType": "custom",
                            "apps": {},
                            "baseUrl": "https://example.com",
                            "apiKey": "",
                        }))
                        .expect("provider json");
                        p.id = format!("t{t}-{i}");
                        db.save_universal_provider(&p).expect("save");
                    }
                });
            }
        });
        assert_eq!(db.get_all_universal_providers().expect("load").len(), 50);
    }
}
