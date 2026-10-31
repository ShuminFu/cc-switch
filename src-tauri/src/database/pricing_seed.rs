//! Built-in model pricing as data (DB-19).
//!
//! The built-in price list lives in `model_pricing.json` next to this file,
//! not in Rust code: changing a price is a JSON edit plus a `seedVersion` bump.
//!
//! Every `model_pricing` row carries `source` (`builtin` | `user`) and
//! `seed_version`. Seeding only ever touches `builtin` rows whose
//! `seed_version` is older than the bundled list; rows a user created or
//! edited (`source = 'user'`, set by [`Database::upsert_model_pricing`]) are
//! never overwritten. Seeding is gated on a device-local settings key, so an
//! up-to-date database issues no writes at all (DB-34).
//!
//! [`Database::upsert_model_pricing`]: super::Database::upsert_model_pricing

use crate::error::AppError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use std::sync::OnceLock;

/// `source` of a row seeded from the bundled list.
pub(crate) const SOURCE_BUILTIN: &str = "builtin";

/// Device-local settings key holding the last `seedVersion` applied here.
pub(crate) const SEED_VERSION_SETTING: &str = "model_pricing_seed_version";

const BUILTIN_PRICING_JSON: &str = include_str!("model_pricing.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuiltinPricing {
    pub(crate) seed_version: i64,
    pub(crate) models: Vec<BuiltinModel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuiltinModel {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) input: String,
    pub(crate) output: String,
    pub(crate) cache_read: String,
    pub(crate) cache_creation: String,
    /// Older built-in values `[input, output, cacheRead, cacheCreation]`: a
    /// row still holding one of them was never edited by the user.
    #[serde(default)]
    pub(crate) previous: Vec<[String; 4]>,
}

impl BuiltinModel {
    fn values(&self) -> [&str; 4] {
        [
            &self.input,
            &self.output,
            &self.cache_read,
            &self.cache_creation,
        ]
    }

    fn is_builtin_value(&self, stored: &[String; 4]) -> bool {
        let current = self.values();
        stored.iter().zip(current).all(|(a, b)| a == b) || self.previous.iter().any(|p| p == stored)
    }
}

/// The bundled price list (parsed once; a malformed file is a build bug that
/// the unit tests catch, at runtime it degrades to an empty list).
pub(crate) fn builtin_pricing() -> &'static BuiltinPricing {
    static PRICING: OnceLock<BuiltinPricing> = OnceLock::new();
    PRICING.get_or_init(|| {
        serde_json::from_str(BUILTIN_PRICING_JSON).unwrap_or_else(|e| {
            log::error!("内置模型定价 model_pricing.json 无法解析: {e}");
            BuiltinPricing {
                seed_version: 0,
                models: Vec::new(),
            }
        })
    })
}

fn db_err(context: &'static str) -> impl Fn(rusqlite::Error) -> AppError {
    move |e| AppError::Database(format!("{context}: {e}"))
}

/// `INSERT OR IGNORE` every built-in model using only the original six
/// columns. Used by historical migrations that run before `source` /
/// `seed_version` exist; the v19→v20 migration then classifies these rows.
pub(crate) fn insert_missing_builtin_rows_legacy(conn: &Connection) -> Result<(), AppError> {
    let mut stmt = conn
        .prepare(
            "INSERT OR IGNORE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .map_err(db_err("准备模型定价语句失败"))?;
    for m in &builtin_pricing().models {
        stmt.execute(params![
            m.id,
            m.name,
            m.input,
            m.output,
            m.cache_read,
            m.cache_creation
        ])
        .map_err(db_err("插入模型定价失败"))?;
    }
    Ok(())
}

/// v19→v20 helper: mark existing rows that still hold a current or previous
/// built-in value as `builtin` (seed_version 0, so the next seeding refreshes
/// them); every other row keeps the column default `user`.
pub(crate) fn classify_existing_rows(conn: &Connection) -> Result<(), AppError> {
    let mut select = conn
        .prepare(
            "SELECT input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
             FROM model_pricing WHERE model_id = ?1",
        )
        .map_err(db_err("读取模型定价失败"))?;
    let mut mark = conn
        .prepare("UPDATE model_pricing SET source = ?1, seed_version = 0 WHERE model_id = ?2")
        .map_err(db_err("标记内置模型定价失败"))?;
    for m in &builtin_pricing().models {
        let stored: Option<[String; 4]> = select
            .query_row([&m.id], |row| {
                Ok([row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?])
            })
            .optional()
            .map_err(db_err("读取模型定价失败"))?;
        if let Some(stored) = stored {
            if m.is_builtin_value(&stored) {
                mark.execute(params![SOURCE_BUILTIN, m.id])
                    .map_err(db_err("标记内置模型定价失败"))?;
            }
        }
    }
    Ok(())
}

fn applied_seed_version(conn: &Connection) -> Result<i64, AppError> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [SEED_VERSION_SETTING],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_err("读取定价种子版本失败"))?;
    Ok(value.and_then(|v| v.trim().parse().ok()).unwrap_or(0))
}

fn has_stale_builtin_rows(conn: &Connection, version: i64) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM model_pricing WHERE source = ?1 AND seed_version < ?2)",
        params![SOURCE_BUILTIN, version],
        |row| row.get(0),
    )
    .map_err(db_err("读取模型定价失败"))
}

/// Whether [`seed_builtin_pricing`] has anything to do (read-only check).
pub(crate) fn needs_seeding(conn: &Connection) -> Result<bool, AppError> {
    let version = builtin_pricing().seed_version;
    Ok(applied_seed_version(conn)? < version || has_stale_builtin_rows(conn, version)?)
}

/// Bring the built-in rows up to the bundled list. Caller wraps it in a
/// transaction/savepoint.
///
/// - First time this device sees `seedVersion` N: missing built-in models are
///   inserted (a model the user deleted earlier comes back only with a new
///   seed version).
/// - `builtin` rows older than N are refreshed (name and prices).
/// - `user` rows are never touched.
pub(crate) fn seed_builtin_pricing(conn: &Connection) -> Result<usize, AppError> {
    let pricing = builtin_pricing();
    let version = pricing.seed_version;
    let first_time_for_version = applied_seed_version(conn)? < version;
    let mut changed = 0;
    {
        let mut insert = conn
            .prepare(
                "INSERT OR IGNORE INTO model_pricing (
                    model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million,
                    source, seed_version
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .map_err(db_err("准备模型定价语句失败"))?;
        let mut refresh = conn
            .prepare(
                "UPDATE model_pricing SET
                    display_name = ?2,
                    input_cost_per_million = ?3,
                    output_cost_per_million = ?4,
                    cache_read_cost_per_million = ?5,
                    cache_creation_cost_per_million = ?6,
                    seed_version = ?7
                 WHERE model_id = ?1 AND source = 'builtin' AND seed_version < ?7",
            )
            .map_err(db_err("准备模型定价语句失败"))?;
        for m in &pricing.models {
            if first_time_for_version {
                changed += insert
                    .execute(params![
                        m.id,
                        m.name,
                        m.input,
                        m.output,
                        m.cache_read,
                        m.cache_creation,
                        SOURCE_BUILTIN,
                        version
                    ])
                    .map_err(db_err("插入模型定价失败"))?;
            }
            changed += refresh
                .execute(params![
                    m.id,
                    m.name,
                    m.input,
                    m.output,
                    m.cache_read,
                    m.cache_creation,
                    version
                ])
                .map_err(db_err("更新模型定价失败"))?;
        }
    }
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SEED_VERSION_SETTING, version.to_string()],
    )
    .map_err(db_err("写入定价种子版本失败"))?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn bundled_pricing_json_is_valid() {
        let pricing: BuiltinPricing =
            serde_json::from_str(BUILTIN_PRICING_JSON).expect("model_pricing.json parses");
        assert!(pricing.seed_version >= 1);
        assert!(pricing.models.len() > 100);
        let mut ids = HashSet::new();
        for m in &pricing.models {
            assert!(ids.insert(m.id.as_str()), "duplicate model id {}", m.id);
            for value in m
                .values()
                .into_iter()
                .chain(m.previous.iter().flatten().map(String::as_str))
            {
                assert!(
                    rust_decimal::Decimal::from_str_exact(value).is_ok(),
                    "{}: price `{value}` is not a decimal",
                    m.id
                );
            }
        }
    }
}
