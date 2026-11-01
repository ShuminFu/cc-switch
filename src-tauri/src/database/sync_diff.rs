//! Table-level diff between two databases for the sync preview (SVC-27).
//!
//! Both sides are compared in their sync-export form (see
//! [`Database::sync_view`]): device-local settings removed and vaulted secrets
//! replaced by placeholders, so only what a download would actually change
//! shows up. The diff names rows and changed key paths only; values are never
//! included, so secrets cannot leak through it.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::types::ValueRef;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

use super::Database;
use crate::error::AppError;

/// Tables shown in the preview, in display order.
pub(crate) const PREVIEW_TABLES: &[&str] =
    &["providers", "mcp_servers", "prompts", "skills", "settings"];

/// Bookkeeping columns that differ between devices without a user change.
const IGNORED_COLUMNS: &[&str] = &["sync_rev", "sync_updated_at"];

/// Columns holding JSON documents, diffed down to key paths.
const JSON_COLUMNS: &[&str] = &["settings_config", "meta", "server_config", "value"];

/// One row that exists on both sides with different content.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedRow {
    pub id: String,
    /// Changed columns, or `column.key.path` inside JSON columns.
    pub fields: Vec<String>,
}

/// Differences of one table, from the local database's point of view:
/// `added` rows exist only on the remote, `removed` rows only locally.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TableDiff {
    pub table: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<ChangedRow>,
}

impl TableDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

type Row = BTreeMap<String, Option<String>>;

impl Database {
    /// This database as a sync export would carry it, opened in memory.
    pub(crate) fn sync_view(&self) -> Result<Database, AppError> {
        let sql = self.export_sql_string_for_sync()?;
        Database::from_sync_export(&sql)
    }

    /// Load a sync export (already verified/decrypted) into a fresh in-memory
    /// database, through the same hardened import path as a real download.
    pub(crate) fn from_sync_export(sql: &str) -> Result<Database, AppError> {
        let db = Database::memory_detached()?;
        db.import_sql_string_inner_no_backup(sql)?;
        Ok(db)
    }
}

/// Diff `local` against `remote` for [`PREVIEW_TABLES`]; only tables with
/// differences are returned.
pub(crate) fn diff_databases(
    local: &Connection,
    remote: &Connection,
) -> Result<Vec<TableDiff>, AppError> {
    let mut diffs = Vec::new();
    for table in PREVIEW_TABLES {
        let diff = diff_table(local, remote, table)?;
        if !diff.is_empty() {
            diffs.push(diff);
        }
    }
    Ok(diffs)
}

fn diff_table(local: &Connection, remote: &Connection, table: &str) -> Result<TableDiff, AppError> {
    let local_rows = load_rows(local, table)?;
    let remote_rows = load_rows(remote, table)?;
    let mut diff = TableDiff {
        table: table.to_string(),
        ..TableDiff::default()
    };
    for (id, remote_row) in &remote_rows {
        match local_rows.get(id) {
            None => diff.added.push(id.clone()),
            Some(local_row) => {
                let fields = changed_fields(local_row, remote_row);
                if !fields.is_empty() {
                    diff.changed.push(ChangedRow {
                        id: id.clone(),
                        fields,
                    });
                }
            }
        }
    }
    diff.removed = local_rows
        .keys()
        .filter(|id| !remote_rows.contains_key(*id))
        .cloned()
        .collect();
    Ok(diff)
}

/// Rows of `table` keyed by their primary key (`a/b` for composite keys).
fn load_rows(conn: &Connection, table: &str) -> Result<BTreeMap<String, Row>, AppError> {
    let db_err = |e: rusqlite::Error| AppError::Database(e.to_string());
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get(0),
        )
        .map_err(db_err)?;
    if !exists {
        return Ok(BTreeMap::new());
    }
    // (name, pk position) in column order.
    let columns: Vec<(String, i64)> = conn
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .and_then(|mut stmt| {
            stmt.query_map([], |row| Ok((row.get(1)?, row.get(5)?)))?
                .collect()
        })
        .map_err(db_err)?;
    let mut key_columns: Vec<(i64, usize)> = columns
        .iter()
        .enumerate()
        .filter(|(_, (_, pk))| *pk > 0)
        .map(|(i, (_, pk))| (*pk, i))
        .collect();
    key_columns.sort();

    let mut stmt = conn
        .prepare(&format!("SELECT * FROM \"{table}\""))
        .map_err(db_err)?;
    let mut rows = stmt.query([]).map_err(db_err)?;
    let mut out = BTreeMap::new();
    while let Some(row) = rows.next().map_err(db_err)? {
        let mut values = Row::new();
        for (i, (name, _)) in columns.iter().enumerate() {
            let value = match row.get_ref(i).map_err(db_err)? {
                ValueRef::Null => None,
                ValueRef::Integer(v) => Some(v.to_string()),
                ValueRef::Real(v) => Some(v.to_string()),
                ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
                ValueRef::Blob(b) => Some(format!(
                    "blob:{}",
                    crate::services::sync_protocol::sha256_hex(b)
                )),
            };
            values.insert(name.clone(), value);
        }
        let id = if key_columns.is_empty() {
            format!("rowid:{}", out.len())
        } else {
            key_columns
                .iter()
                .map(|(_, i)| values[&columns[*i].0].clone().unwrap_or_default())
                .collect::<Vec<_>>()
                .join("/")
        };
        out.insert(id, values);
    }
    Ok(out)
}

fn changed_fields(local: &Row, remote: &Row) -> Vec<String> {
    let names: BTreeSet<&String> = local.keys().chain(remote.keys()).collect();
    let mut fields = Vec::new();
    for name in names {
        if IGNORED_COLUMNS.contains(&name.as_str()) {
            continue;
        }
        let (a, b) = (
            local.get(name).cloned().flatten(),
            remote.get(name).cloned().flatten(),
        );
        if a == b {
            continue;
        }
        if JSON_COLUMNS.contains(&name.as_str()) {
            if let (Some(Ok(a)), Some(Ok(b))) = (
                a.as_deref().map(serde_json::from_str::<Value>),
                b.as_deref().map(serde_json::from_str::<Value>),
            ) {
                let mut paths = Vec::new();
                json_paths(name, &a, &b, &mut paths);
                fields.extend(paths);
                continue;
            }
        }
        fields.push(name.clone());
    }
    fields
}

/// Key paths under `prefix` where `a` and `b` differ (objects are descended;
/// arrays and scalars are compared whole).
fn json_paths(prefix: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            for key in keys {
                let path = format!("{prefix}.{key}");
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => json_paths(&path, x, y, out),
                    _ => out.push(path),
                }
            }
        }
        _ if a == b => {}
        _ => out.push(prefix.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use serde_json::json;

    fn provider(id: &str, base_url: &str, key: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            format!("Provider {id}"),
            json!({ "env": {
                "ANTHROPIC_BASE_URL": base_url,
                "ANTHROPIC_AUTH_TOKEN": key,
            }}),
            None,
        )
    }

    fn diff_of(local: &Database, remote: &Database) -> Vec<TableDiff> {
        let local = local.sync_view().unwrap();
        let remote = remote.sync_view().unwrap();
        let l = local.conn.lock().unwrap();
        let r = remote.conn.lock().unwrap();
        diff_databases(&l, &r).unwrap()
    }

    #[test]
    fn identical_snapshots_have_an_empty_diff() {
        let db = Database::memory().unwrap();
        db.save_provider("claude", &provider("p1", "https://a.example", "sk-secret"))
            .unwrap();
        let remote = Database::from_sync_export(&db.export_sql_string_for_sync().unwrap()).unwrap();
        assert!(diff_of(&db, &remote).is_empty());
    }

    #[test]
    fn changed_base_url_names_the_key_path_and_never_the_value() {
        let remote = Database::memory().unwrap();
        remote
            .save_provider(
                "claude",
                &provider("p1", "https://a.example", "sk-remote-secret"),
            )
            .unwrap();
        remote
            .save_provider("claude", &provider("p2", "https://b.example", "sk-2"))
            .unwrap();
        let local = Database::memory().unwrap();
        local
            .save_provider(
                "claude",
                &provider("p1", "https://changed.example", "sk-remote-secret"),
            )
            .unwrap();
        local
            .save_provider("claude", &provider("p3", "https://c.example", "sk-3"))
            .unwrap();

        let diffs = diff_of(&local, &remote);
        assert_eq!(diffs.len(), 1, "{diffs:?}");
        let providers = &diffs[0];
        assert_eq!(providers.table, "providers");
        assert_eq!(providers.added, vec!["p2/claude".to_string()]);
        assert_eq!(providers.removed, vec!["p3/claude".to_string()]);
        assert_eq!(
            providers.changed,
            vec![ChangedRow {
                id: "p1/claude".to_string(),
                fields: vec!["settings_config.env.ANTHROPIC_BASE_URL".to_string()],
            }]
        );
        let rendered = serde_json::to_string(&diffs).unwrap();
        assert!(!rendered.contains("sk-"), "values never appear: {rendered}");
        assert!(!rendered.contains("example"), "{rendered}");
    }

    #[test]
    fn json_paths_reports_added_removed_and_nested_keys() {
        let mut out = Vec::new();
        json_paths(
            "c",
            &json!({"a": 1, "b": {"x": 1, "y": [1]}, "gone": true}),
            &json!({"a": 1, "b": {"x": 2, "y": [1, 2]}, "new": null}),
            &mut out,
        );
        assert_eq!(out, vec!["c.b.x", "c.b.y", "c.gone", "c.new"]);
    }
}
