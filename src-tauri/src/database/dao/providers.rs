use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta};
use crate::secrets::provider_vault::{self, ProviderRow};
use indexmap::IndexMap;
use rusqlite::params;
use std::collections::{HashMap, HashSet};

type OmoProviderRow = (
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<usize>,
    Option<String>,
    String,
);

/// A provider row whose JSON columns could not be parsed (DB-24). Such rows
/// are skipped by list queries instead of being coerced to `null`/defaults
/// (which a later save would write back over the original bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorruptProviderRow {
    pub id: String,
    pub column: &'static str,
    pub error: String,
}

/// Parse a provider row's `settings_config` and `meta` JSON strictly.
fn parse_provider_json(
    settings_config: &str,
    meta: &str,
) -> Result<(serde_json::Value, ProviderMeta), (&'static str, String)> {
    // Blank columns carry no data to lose: keep the historical defaults.
    let settings_config = if settings_config.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(settings_config).map_err(|e| ("settings_config", e.to_string()))?
    };
    let meta = if meta.trim().is_empty() {
        ProviderMeta::default()
    } else {
        serde_json::from_str(meta).map_err(|e| ("meta", e.to_string()))?
    };
    Ok((settings_config, meta))
}

impl Database {
    pub fn get_all_providers(
        &self,
        app_type: &str,
    ) -> Result<IndexMap<String, Provider>, AppError> {
        let (providers, corrupt) = self.get_all_providers_checked(app_type)?;
        for row in &corrupt {
            log::warn!(
                "[DB] Skipping provider '{}' ({app_type}): column {} is not valid JSON: {}",
                row.id,
                row.column,
                row.error
            );
        }
        Ok(providers)
    }

    /// [`Self::get_all_providers`] plus the rows that were skipped because a
    /// JSON column does not parse (DB-24).
    ///
    /// DB-21: endpoints are loaded with ONE query for the whole app and grouped
    /// in Rust (no per-provider query).
    pub fn get_all_providers_checked(
        &self,
        app_type: &str,
    ) -> Result<(IndexMap<String, Provider>, Vec<CorruptProviderRow>), AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, settings_config, website_url, category, created_at, sort_index, notes, icon, icon_color, meta, in_failover_queue
             FROM providers WHERE app_type = ?1
             ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let provider_iter = stmt
            .query_map(params![app_type], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let settings_config_str: String = row.get(2)?;
                let website_url: Option<String> = row.get(3)?;
                let category: Option<String> = row.get(4)?;
                let created_at: Option<i64> = row.get(5)?;
                let sort_index: Option<usize> = row.get(6)?;
                let notes: Option<String> = row.get(7)?;
                let icon: Option<String> = row.get(8)?;
                let icon_color: Option<String> = row.get(9)?;
                let meta_str: String = row.get(10)?;
                let in_failover_queue: bool = row.get(11)?;

                let parsed = parse_provider_json(&settings_config_str, &meta_str).map(
                    |(settings_config, meta)| Provider {
                        id: id.clone(),
                        name,
                        settings_config,
                        website_url,
                        category,
                        created_at,
                        sort_index,
                        notes,
                        meta: Some(meta),
                        icon,
                        icon_color,
                        in_failover_queue,
                    },
                );
                Ok((id, parsed))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut providers = IndexMap::new();
        let mut corrupt = Vec::new();
        for provider_res in provider_iter {
            let (id, parsed) = provider_res.map_err(|e| AppError::Database(e.to_string()))?;
            let mut provider = match parsed {
                Ok(provider) => provider,
                Err((column, error)) => {
                    corrupt.push(CorruptProviderRow { id, column, error });
                    continue;
                }
            };
            // DB-02: callers always receive real values, never placeholders.
            provider.settings_config = provider_vault::for_read(
                app_type,
                &id,
                std::mem::take(&mut provider.settings_config),
            );
            providers.insert(id, provider);
        }
        drop(stmt);

        let mut endpoints_by_provider: HashMap<
            String,
            HashMap<String, crate::settings::CustomEndpoint>,
        > = HashMap::new();
        {
            let mut stmt_endpoints = conn
                .prepare(
                    "SELECT provider_id, url, added_at FROM provider_endpoints
                     WHERE app_type = ?1 ORDER BY added_at ASC, url ASC",
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            let endpoints_iter = stmt_endpoints
                .query_map(params![app_type], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                })
                .map_err(|e| AppError::Database(e.to_string()))?;
            for ep_res in endpoints_iter {
                let (provider_id, url, added_at) =
                    ep_res.map_err(|e| AppError::Database(e.to_string()))?;
                endpoints_by_provider
                    .entry(provider_id)
                    .or_default()
                    .insert(
                        url.clone(),
                        crate::settings::CustomEndpoint {
                            url,
                            added_at: added_at.unwrap_or(0),
                            last_used: None,
                        },
                    );
            }
        }

        for (id, provider) in providers.iter_mut() {
            if let Some(meta) = &mut provider.meta {
                meta.custom_endpoints = endpoints_by_provider.remove(id).unwrap_or_default();
            }
        }

        Ok((providers, corrupt))
    }

    pub fn get_current_provider(&self, app_type: &str) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT id FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut rows = stmt
            .query(params![app_type])
            .map_err(|e| AppError::Database(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(
                row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
            ))
        } else {
            Ok(None)
        }
    }

    pub fn get_provider_by_id(
        &self,
        id: &str,
        app_type: &str,
    ) -> Result<Option<Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT name, settings_config, website_url, category, created_at, sort_index, notes, icon, icon_color, meta, in_failover_queue
             FROM providers WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
            |row| {
                let name: String = row.get(0)?;
                let settings_config_str: String = row.get(1)?;
                let website_url: Option<String> = row.get(2)?;
                let category: Option<String> = row.get(3)?;
                let created_at: Option<i64> = row.get(4)?;
                let sort_index: Option<usize> = row.get(5)?;
                let notes: Option<String> = row.get(6)?;
                let icon: Option<String> = row.get(7)?;
                let icon_color: Option<String> = row.get(8)?;
                let meta_str: String = row.get(9)?;
                let in_failover_queue: bool = row.get(10)?;

                // DB-24: a corrupt row is an error, never a provider with a
                // `null` config that a later save would persist.
                let (settings_config, meta) = parse_provider_json(&settings_config_str, &meta_str)
                    .map_err(|(column, error)| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            format!("provider '{id}' column {column} is not valid JSON: {error}")
                                .into(),
                        )
                    })?;

                Ok(Provider {
                    id: id.to_string(),
                    name,
                    settings_config,
                    website_url,
                    category,
                    created_at,
                    sort_index,
                    notes,
                    meta: Some(meta),
                    icon,
                    icon_color,
                    in_failover_queue,
                })
            },
        );

        match result {
            Ok(mut provider) => {
                provider.settings_config = provider_vault::for_read(
                    app_type,
                    id,
                    std::mem::take(&mut provider.settings_config),
                );
                Ok(Some(provider))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn save_provider(&self, app_type: &str, provider: &Provider) -> Result<(), AppError> {
        // DB-02: with the vault on, secrets go to the keychain and the row keeps
        // placeholders. Keychain writes happen before the row is written.
        let stored_config =
            provider_vault::for_storage(app_type, &provider.id, &provider.settings_config);
        let stored_config_json = serde_json::to_string(&stored_config)
            .map_err(|e| AppError::Database(format!("Failed to serialize settings_config: {e}")))?;

        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        let previous_config = Self::raw_settings_config(&tx, app_type, &provider.id);

        let mut meta_clone = provider.meta.clone().unwrap_or_default();
        let endpoints = std::mem::take(&mut meta_clone.custom_endpoints);

        let existing: Option<(bool, bool)> = tx
            .query_row(
                "SELECT is_current, in_failover_queue FROM providers WHERE id = ?1 AND app_type = ?2",
                params![provider.id, app_type],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        let is_update = existing.is_some();
        let (is_current, in_failover_queue) =
            existing.unwrap_or((false, provider.in_failover_queue));

        // DB-24: never overwrite a row whose stored JSON does not parse; the
        // caller's struct was not loaded from it, and the original bytes may
        // still be recoverable.
        if is_update {
            if let Some((column, error)) = Self::corrupt_json_column(&tx, app_type, &provider.id) {
                return Err(AppError::Database(format!(
                    "供应商 '{}' 的 {column} 数据已损坏，拒绝覆盖（请先修复或删除该行）: {error}",
                    provider.id
                )));
            }
        }

        if is_update {
            tx.execute(
                "UPDATE providers SET
                    name = ?1,
                    settings_config = ?2,
                    website_url = ?3,
                    category = ?4,
                    created_at = ?5,
                    sort_index = ?6,
                    notes = ?7,
                    icon = ?8,
                    icon_color = ?9,
                    meta = ?10,
                    is_current = ?11,
                    in_failover_queue = ?12
                WHERE id = ?13 AND app_type = ?14",
                params![
                    provider.name,
                    stored_config_json,
                    provider.website_url,
                    provider.category,
                    provider.created_at,
                    provider.sort_index,
                    provider.notes,
                    provider.icon,
                    provider.icon_color,
                    serde_json::to_string(&meta_clone).map_err(|e| AppError::Database(format!(
                        "Failed to serialize meta: {e}"
                    )))?,
                    is_current,
                    in_failover_queue,
                    provider.id,
                    app_type,
                ],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        } else {
            tx.execute(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, website_url, category,
                    created_at, sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    provider.id,
                    app_type,
                    provider.name,
                    stored_config_json,
                    provider.website_url,
                    provider.category,
                    provider.created_at,
                    provider.sort_index,
                    provider.notes,
                    provider.icon,
                    provider.icon_color,
                    serde_json::to_string(&meta_clone)
                        .map_err(|e| AppError::Database(format!("Failed to serialize meta: {e}")))?,
                    is_current,
                    in_failover_queue,
                ],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

            for (url, endpoint) in endpoints {
                tx.execute(
                    "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(provider_id, app_type, url) DO NOTHING",
                    params![provider.id, app_type, url, endpoint.added_at],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            }
        }

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        drop(conn);
        if let Some(previous) = previous_config {
            provider_vault::forget_stale(app_type, &provider.id, &previous, Some(&stored_config));
        }
        Ok(())
    }

    /// The first JSON column of a stored provider row that does not parse.
    fn corrupt_json_column(
        conn: &rusqlite::Connection,
        app_type: &str,
        id: &str,
    ) -> Option<(&'static str, String)> {
        let (settings_config, meta): (String, String) = conn
            .query_row(
                "SELECT settings_config, meta FROM providers WHERE id = ?1 AND app_type = ?2",
                params![id, app_type],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok()?;
        parse_provider_json(&settings_config, &meta).err()
    }

    /// The stored (unresolved) `settings_config` of one row.
    fn raw_settings_config(
        conn: &rusqlite::Connection,
        app_type: &str,
        id: &str,
    ) -> Option<serde_json::Value> {
        conn.query_row(
            "SELECT settings_config FROM providers WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn delete_provider(&self, app_type: &str, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let previous_config = Self::raw_settings_config(&conn, app_type, id);
        conn.execute(
            "DELETE FROM providers WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        drop(conn);
        // DB-02: a deleted provider leaves no keychain entries behind.
        if let Some(previous) = previous_config {
            provider_vault::forget_stale(app_type, id, &previous, None);
        }
        Ok(())
    }

    /// Every provider row with its stored (unresolved) `settings_config`, for
    /// the vault migrations and status (DB-02).
    pub fn load_provider_secret_rows(&self) -> Result<Vec<ProviderRow>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT app_type, id, settings_config FROM providers ORDER BY app_type, id")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            let (app, id, json) = row.map_err(|e| AppError::Database(e.to_string()))?;
            // Rows that do not parse are left alone (never rewritten).
            if let Ok(settings_config) = serde_json::from_str(&json) {
                out.push(ProviderRow {
                    app,
                    id,
                    settings_config,
                });
            }
        }
        Ok(out)
    }

    /// Write back `settings_config` of the given rows as-is (no vault
    /// processing), in one transaction.
    pub fn store_provider_secret_rows(&self, rows: &[ProviderRow]) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        for row in rows {
            let json = serde_json::to_string(&row.settings_config).map_err(|e| {
                AppError::Database(format!("Failed to serialize settings_config: {e}"))
            })?;
            tx.execute(
                "UPDATE providers SET settings_config = ?1 WHERE id = ?2 AND app_type = ?3",
                params![json, row.id, row.app],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn set_current_provider(&self, app_type: &str, id: &str) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "UPDATE providers SET is_current = 0 WHERE app_type = ?1",
            params![app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn update_provider_settings_config(
        &self,
        app_type: &str,
        provider_id: &str,
        settings_config: &serde_json::Value,
    ) -> Result<(), AppError> {
        let stored_config = provider_vault::for_storage(app_type, provider_id, settings_config);
        let conn = lock_conn!(self.conn);
        let previous_config = Self::raw_settings_config(&conn, app_type, provider_id);
        conn.execute(
            "UPDATE providers SET settings_config = ?1 WHERE id = ?2 AND app_type = ?3",
            params![
                serde_json::to_string(&stored_config).map_err(|e| AppError::Database(format!(
                    "Failed to serialize settings_config: {e}"
                )))?,
                provider_id,
                app_type
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        drop(conn);
        if let Some(previous) = previous_config {
            provider_vault::forget_stale(app_type, provider_id, &previous, Some(&stored_config));
        }
        Ok(())
    }

    pub fn add_custom_endpoint(
        &self,
        app_type: &str,
        provider_id: &str,
        url: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let added_at = chrono::Utc::now().timestamp_millis();
        conn.execute(
            // DB-22: adding an existing URL again is a no-op.
            "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider_id, app_type, url) DO NOTHING",
            params![provider_id, app_type, url, added_at],
        ).map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn remove_custom_endpoint(
        &self,
        app_type: &str,
        provider_id: &str,
        url: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2 AND url = ?3",
            params![provider_id, app_type, url],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn set_omo_provider_current(
        &self,
        app_type: &str,
        provider_id: &str,
        category: &str,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute(
            "UPDATE providers SET is_current = 0 WHERE app_type = ?1 AND category = ?2",
            params![app_type, category],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        // OMO ↔ OMO Slim mutually exclusive: deactivate the opposite category
        let opposite = match category {
            "omo" => Some("omo-slim"),
            "omo-slim" => Some("omo"),
            _ => None,
        };
        if let Some(opp) = opposite {
            tx.execute(
                "UPDATE providers SET is_current = 0 WHERE app_type = ?1 AND category = ?2",
                params![app_type, opp],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        let updated = tx
            .execute(
                "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2 AND category = ?3",
                params![provider_id, app_type, category],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        if updated != 1 {
            return Err(AppError::Database(format!(
                "Failed to set {category} provider current: provider '{provider_id}' not found in app '{app_type}'"
            )));
        }
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn is_omo_provider_current(
        &self,
        app_type: &str,
        provider_id: &str,
        category: &str,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        match conn.query_row(
            "SELECT is_current FROM providers
             WHERE id = ?1 AND app_type = ?2 AND category = ?3",
            params![provider_id, app_type, category],
            |row| row.get(0),
        ) {
            Ok(is_current) => Ok(is_current),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn clear_omo_provider_current(
        &self,
        app_type: &str,
        provider_id: &str,
        category: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE providers SET is_current = 0
             WHERE id = ?1 AND app_type = ?2 AND category = ?3",
            params![provider_id, app_type, category],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_current_omo_provider(
        &self,
        app_type: &str,
        category: &str,
    ) -> Result<Option<Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        let row_data: Result<OmoProviderRow, rusqlite::Error> = conn.query_row(
            "SELECT id, name, settings_config, category, created_at, sort_index, notes, meta
             FROM providers
             WHERE app_type = ?1 AND category = ?2 AND is_current = 1
             LIMIT 1",
            params![app_type, category],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        );

        let (id, name, settings_config_str, _row_category, created_at, sort_index, notes, meta_str) =
            match row_data {
                Ok(v) => v,
                Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
                Err(e) => return Err(AppError::Database(e.to_string())),
            };

        let settings_config = serde_json::from_str(&settings_config_str).map_err(|e| {
            AppError::Database(format!(
                "Failed to parse {category} provider settings_config (provider_id={id}): {e}"
            ))
        })?;
        let meta: crate::provider::ProviderMeta = if meta_str.trim().is_empty() {
            crate::provider::ProviderMeta::default()
        } else {
            serde_json::from_str(&meta_str).map_err(|e| {
                AppError::Database(format!(
                    "Failed to parse {category} provider meta (provider_id={id}): {e}"
                ))
            })?
        };

        let settings_config = provider_vault::for_read(app_type, &id, settings_config);
        Ok(Some(Provider {
            id,
            name,
            settings_config,
            website_url: None,
            category: Some(category.to_string()),
            created_at,
            sort_index,
            notes,
            meta: Some(meta),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }))
    }

    /// 判断 providers 表是否为空（全 app_type 一起算）。
    ///
    /// 用于区分"全新安装"和"升级用户"：在启动流程 import/seed 之前调用。
    /// 使用 `EXISTS` 短路查询，比 `COUNT(*)` 在将来表变大时更高效。
    pub fn is_providers_empty(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let exists: bool = conn
            .query_row("SELECT EXISTS(SELECT 1 FROM providers)", [], |row| {
                row.get(0)
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(!exists)
    }

    /// 仅获取指定 app 下所有 provider 的 id 集合。
    ///
    /// 比 `get_all_providers` 轻量得多：只读 id 列、无 endpoint 子查询。
    /// 用于只需要做存在性检查的场景（如 additive 模式的 live 同步去重）。
    pub fn get_provider_ids(&self, app_type: &str) -> Result<HashSet<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT id FROM providers WHERE app_type = ?1")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![app_type], |row| row.get::<_, String>(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut ids = HashSet::new();
        for row in rows {
            ids.insert(row.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(ids)
    }

    /// 判断指定 app 下是否已存在任意 provider。
    ///
    /// 启动阶段的 live import 需要使用这个更严格的判断：
    /// 只要该 app 已经有任何 provider（包括官方 seed），就不应再自动导入 `default`。
    pub fn has_any_provider_for_app(&self, app_type: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM providers WHERE app_type = ?1)",
                params![app_type],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(exists)
    }

    /// 判断指定 app 下是否存在非官方种子的供应商。
    ///
    /// 比 `get_all_providers` 轻量得多：只读 id 列、无 endpoint 子查询、首条命中即返回。
    /// 用于 `import_default_config` 决定是否跳过 live 导入。
    pub fn has_non_official_seed_provider(&self, app_type: &str) -> Result<bool, AppError> {
        use crate::database::dao::providers_seed::is_official_seed_id;
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT id FROM providers WHERE app_type = ?1")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![app_type])
            .map_err(|e| AppError::Database(e.to_string()))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let id: String = row.get(0).map_err(|e| AppError::Database(e.to_string()))?;
            if !is_official_seed_id(&id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 计算指定 app 下一个可用的 sort_index（追加到末尾）。
    fn next_sort_index_for_app(&self, app_type: &str) -> Result<usize, AppError> {
        let conn = lock_conn!(self.conn);
        let max: Option<i64> = conn
            .query_row(
                "SELECT MAX(sort_index) FROM providers WHERE app_type = ?1",
                params![app_type],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(max.map(|v| (v + 1) as usize).unwrap_or(0))
    }

    /// 启动时调用：补齐缺失的官方预设供应商（Claude / Codex / Gemini）。
    ///
    /// 使用 settings flag `official_providers_seeded` 保证每个数据库只执行一次：
    /// - 全新用户：seed 三条官方预设
    /// - 老用户升级：同样会触发一次（flag 不存在），追加到末尾，不影响已有排序
    /// - 用户删除 seed 后：不再重建（flag 已为 true），尊重用户意图
    ///
    /// 与 `Database::save_provider` 的 UPSERT 语义配合，即使被意外重复调用
    /// 也不会覆盖用户当前激活的供应商（is_current 字段会被保留）。
    pub fn init_default_official_providers(&self) -> Result<usize, AppError> {
        use crate::database::dao::providers_seed::OFFICIAL_SEEDS;

        if self
            .get_bool_flag("official_providers_seeded")
            .unwrap_or(false)
        {
            return Ok(0);
        }

        let mut inserted = 0_usize;
        let now_ms = chrono::Utc::now().timestamp_millis();

        for seed in OFFICIAL_SEEDS {
            let app_type_str = seed.app_type.as_str();

            // 若该 id 已存在（极端情况：用户曾手动用过同 id），跳过
            if self.get_provider_by_id(seed.id, app_type_str)?.is_some() {
                continue;
            }

            let next_sort_index = self.next_sort_index_for_app(app_type_str)?;

            let settings_config: serde_json::Value =
                serde_json::from_str(seed.settings_config_json).map_err(|e| {
                    AppError::Database(format!("Seed JSON parse failed for {}: {e}", seed.id))
                })?;

            let mut provider = Provider::with_id(
                seed.id.to_string(),
                seed.name.to_string(),
                settings_config,
                Some(seed.website_url.to_string()),
            );
            provider.category = Some("official".to_string());
            provider.icon = Some(seed.icon.to_string());
            provider.icon_color = Some(seed.icon_color.to_string());
            provider.sort_index = Some(next_sort_index);
            provider.created_at = Some(now_ms);

            self.save_provider(app_type_str, &provider)?;
            inserted += 1;
            log::info!(
                "✓ Seeded official provider: {} ({})",
                seed.name,
                app_type_str
            );
        }

        // 即使 inserted=0（例如用户手动创建过同 id）也设置 flag 防止反复检查
        self.set_setting("official_providers_seeded", "true")?;

        Ok(inserted)
    }

    /// 按 id 兜底插入单条 official seed（仅当目标表中该 id 不存在时插入）。
    ///
    /// 与 `init_default_official_providers` 不同：
    /// - 不触碰 `official_providers_seeded` 全局 flag，是 on-demand 修复
    /// - 只处理一条 seed，由调用方决定 id + app_type
    /// - 已存在则尊重用户自定义，不覆盖
    ///
    /// 返回 Ok(true) 表示插入了新行，Ok(false) 表示已存在被跳过。
    pub fn ensure_official_seed_by_id(
        &self,
        seed_id: &str,
        app_type: crate::app_config::AppType,
    ) -> Result<bool, AppError> {
        use crate::database::dao::providers_seed::OFFICIAL_SEEDS;

        let seed = OFFICIAL_SEEDS
            .iter()
            .find(|s| s.id == seed_id && s.app_type == app_type)
            .ok_or_else(|| {
                AppError::Database(format!(
                    "unknown official seed: id={seed_id}, app_type={}",
                    app_type.as_str()
                ))
            })?;

        let app_type_str = seed.app_type.as_str();

        if self.get_provider_by_id(seed_id, app_type_str)?.is_some() {
            return Ok(false);
        }

        let settings_config: serde_json::Value = serde_json::from_str(seed.settings_config_json)
            .map_err(|e| {
                AppError::Database(format!("Seed JSON parse failed for {}: {e}", seed.id))
            })?;

        let next_sort_index = self.next_sort_index_for_app(app_type_str)?;
        let now_ms = chrono::Utc::now().timestamp_millis();

        let mut provider = Provider::with_id(
            seed.id.to_string(),
            seed.name.to_string(),
            settings_config,
            Some(seed.website_url.to_string()),
        );
        provider.category = Some("official".to_string());
        provider.icon = Some(seed.icon.to_string());
        provider.icon_color = Some(seed.icon_color.to_string());
        provider.sort_index = Some(next_sort_index);
        provider.created_at = Some(now_ms);

        self.save_provider(app_type_str, &provider)?;

        Ok(true)
    }
}

#[cfg(test)]
mod row_integrity_tests {
    use crate::database::{lock_conn, Database};
    use crate::error::AppError;
    use crate::provider::Provider;
    use serde_json::json;

    fn insert_raw(db: &Database, id: &str, settings_config: &str) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES (?1, 'claude', ?1, ?2, '{}')",
            rusqlite::params![id, settings_config],
        )?;
        Ok(())
    }

    /// DB-24: a corrupt row is skipped (and reported), never coerced to `null`,
    /// and a save refuses to overwrite it.
    #[test]
    fn corrupt_provider_rows_are_quarantined_not_coerced() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_raw(&db, "good", r#"{"env":{}}"#)?;
        insert_raw(&db, "bad", "{broken")?;

        let providers = db.get_all_providers("claude")?;
        assert!(providers.contains_key("good"));
        assert!(!providers.contains_key("bad"));

        let (_, corrupt) = db.get_all_providers_checked("claude")?;
        assert_eq!(corrupt.len(), 1);
        assert_eq!(corrupt[0].id, "bad");
        assert_eq!(corrupt[0].column, "settings_config");

        assert!(db.get_provider_by_id("bad", "claude").is_err());

        let replacement = Provider::with_id("bad".into(), "bad".into(), json!({}), None);
        assert!(db.save_provider("claude", &replacement).is_err());
        let conn = lock_conn!(db.conn);
        let stored: String = conn.query_row(
            "SELECT settings_config FROM providers WHERE id = 'bad'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(stored, "{broken", "original bytes are kept");
        Ok(())
    }

    /// DB-21: endpoints are grouped per provider from one query.
    #[test]
    fn endpoints_are_attached_to_their_own_provider() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_raw(&db, "a", "{}")?;
        insert_raw(&db, "b", "{}")?;
        db.add_custom_endpoint("claude", "a", "https://a1.example")?;
        db.add_custom_endpoint("claude", "a", "https://a2.example")?;
        db.add_custom_endpoint("claude", "b", "https://b1.example")?;

        let providers = db.get_all_providers("claude")?;
        let urls = |id: &str| {
            let mut v: Vec<String> = providers[id]
                .meta
                .as_ref()
                .unwrap()
                .custom_endpoints
                .keys()
                .cloned()
                .collect();
            v.sort();
            v
        };
        assert_eq!(urls("a"), vec!["https://a1.example", "https://a2.example"]);
        assert_eq!(urls("b"), vec!["https://b1.example"]);
        Ok(())
    }
}

#[cfg(test)]
mod ensure_official_seed_tests {
    use crate::app_config::AppType;
    use crate::database::{
        Database, CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, CODEX_OFFICIAL_PROVIDER_ID,
    };

    #[test]
    fn ensure_inserts_when_missing() {
        let db = Database::memory().expect("memory db");
        let inserted = db
            .ensure_official_seed_by_id(CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, AppType::ClaudeDesktop)
            .expect("ensure ok");
        assert!(inserted, "should insert when missing");

        let provider = db
            .get_provider_by_id(
                CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                AppType::ClaudeDesktop.as_str(),
            )
            .expect("query ok")
            .expect("provider exists after ensure");

        assert_eq!(provider.id, CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID);
        assert_eq!(provider.name, "Claude Desktop Official");
        assert_eq!(provider.category.as_deref(), Some("official"));
        assert_eq!(provider.icon.as_deref(), Some("anthropic"));
        assert_eq!(provider.icon_color.as_deref(), Some("#D4915D"));
    }

    #[test]
    fn ensure_skips_when_present_and_preserves_customization() {
        let db = Database::memory().expect("memory db");
        db.init_default_official_providers().expect("seed");

        let mut renamed = db
            .get_provider_by_id(
                CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                AppType::ClaudeDesktop.as_str(),
            )
            .expect("query ok")
            .expect("seed present");
        renamed.name = "My Custom Backup".to_string();
        db.save_provider(AppType::ClaudeDesktop.as_str(), &renamed)
            .expect("save customization");

        let inserted = db
            .ensure_official_seed_by_id(CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, AppType::ClaudeDesktop)
            .expect("ensure ok");
        assert!(!inserted, "should skip when present");

        let after = db
            .get_provider_by_id(
                CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                AppType::ClaudeDesktop.as_str(),
            )
            .expect("query ok")
            .expect("still present");
        assert_eq!(
            after.name, "My Custom Backup",
            "customization must not be overwritten"
        );
    }

    #[test]
    fn ensure_recreates_codex_official_seed_after_deletion() {
        let db = Database::memory().expect("memory db");
        db.init_default_official_providers().expect("seed");
        db.delete_provider(AppType::Codex.as_str(), CODEX_OFFICIAL_PROVIDER_ID)
            .expect("delete Codex official");

        let inserted = db
            .ensure_official_seed_by_id(CODEX_OFFICIAL_PROVIDER_ID, AppType::Codex)
            .expect("ensure Codex official");
        assert!(inserted);
        let provider = db
            .get_provider_by_id(CODEX_OFFICIAL_PROVIDER_ID, AppType::Codex.as_str())
            .expect("query")
            .expect("Codex official restored");
        assert_eq!(provider.category.as_deref(), Some("official"));
        assert_eq!(provider.settings_config["auth"], serde_json::json!({}));
    }

    #[test]
    fn ensure_rejects_unknown_seed() {
        let db = Database::memory().expect("memory db");
        let result = db.ensure_official_seed_by_id("nonexistent-id", AppType::ClaudeDesktop);
        assert!(result.is_err(), "unknown seed id should be Err");
    }

    #[test]
    fn ensure_rejects_seed_app_type_mismatch() {
        let db = Database::memory().expect("memory db");
        let result =
            db.ensure_official_seed_by_id(CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, AppType::Claude);
        assert!(result.is_err(), "(id, app_type) mismatch should be Err");
    }
}
