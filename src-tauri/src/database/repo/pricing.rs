//! 定价仓库：`model_pricing` 表的读写 SQL
//!
//! 解析规则（模型名归一化、候选顺序）在 [`crate::pricing::resolver`]，目录同步与
//! 来源（provenance）规则在 [`crate::pricing::catalog`]；这里只执行语句。
//! 读路径一律过滤墓碑（`deleted = 0`）。

use crate::error::AppError;
use crate::pricing::catalog::{Catalog, CatalogModel, CatalogPrice, SOURCE_BUILTIN, SOURCE_USER};
use rusqlite::{params, Connection, OptionalExtension};

/// 每百万 token 单价：(input, output, cache_read, cache_creation)
pub(crate) type PricingRow = (String, String, String, String);

/// `get_model_pricing` 列表中的一行
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PricingListRow {
    pub model_id: String,
    pub display_name: String,
    pub price: CatalogPrice,
}

fn map_pricing_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PricingRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

/// 按 model_id 精确查价。
pub(crate) fn find_exact(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<PricingRow>, AppError> {
    conn.query_row(
        "SELECT input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         WHERE model_id = ?1 AND deleted = 0",
        [model_id],
        map_pricing_row,
    )
    .optional()
    .map_err(|e| AppError::Database(format!("查询模型定价失败: {e}")))
}

/// 按 `{model_id}-%` 前缀查价，取最短的 model_id。
pub(crate) fn find_prefix(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<PricingRow>, AppError> {
    let pattern = format!("{model_id}-%");
    conn.query_row(
        "SELECT input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         WHERE model_id LIKE ?1 AND deleted = 0
         ORDER BY LENGTH(model_id) ASC
         LIMIT 1",
        [pattern],
        map_pricing_row,
    )
    .optional()
    .map_err(|e| AppError::Database(format!("查询模型前缀定价失败: {e}")))
}

/// 未删除的定价行，按显示名排序。表不存在（迁移未执行）时返回 `None`。
pub(crate) fn list_active(conn: &Connection) -> Result<Option<Vec<PricingListRow>>, AppError> {
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='model_pricing'",
            [],
            |row| row.get::<_, i64>(0).map(|count| count > 0),
        )
        .unwrap_or(false);
    if !table_exists {
        return Ok(None);
    }

    let mut stmt = conn.prepare(
        "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         WHERE deleted = 0
         ORDER BY display_name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PricingListRow {
            model_id: row.get(0)?,
            display_name: row.get(1)?,
            price: CatalogPrice {
                input: row.get(2)?,
                output: row.get(3)?,
                cache_read: row.get(4)?,
                cache_creation: row.get(5)?,
            },
        })
    })?;
    let mut pricing = Vec::new();
    for row in rows {
        pricing.push(row?);
    }
    Ok(Some(pricing))
}

/// v18 之前的 seed：只写基础列，`INSERT OR IGNORE`。返回处理的模型数。
pub(crate) fn insert_legacy_seed<'a>(
    conn: &Connection,
    models: impl IntoIterator<Item = &'a CatalogModel>,
) -> Result<usize, AppError> {
    let mut stmt = conn
        .prepare(
            "INSERT OR IGNORE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .map_err(|e| AppError::Database(format!("准备模型定价语句失败: {e}")))?;

    let mut count = 0usize;
    for model in models {
        let p = &model.price;
        stmt.execute(params![
            model.id,
            model.name,
            p.input,
            p.output,
            p.cache_read,
            p.cache_creation
        ])
        .map_err(|e| AppError::Database(format!("插入模型定价失败: {e}")))?;
        count += 1;
    }
    Ok(count)
}

/// 价格（以及给定时的显示名）与 `price` 一致的行标为 `builtin`。返回更新的行数。
pub(crate) fn mark_builtin_if_unchanged(
    conn: &Connection,
    model_id: &str,
    price: &CatalogPrice,
    name: Option<&str>,
    version: Option<u32>,
) -> Result<usize, AppError> {
    conn.execute(
        "UPDATE model_pricing SET source = ?2, catalog_version = ?3
         WHERE model_id = ?1
           AND input_cost_per_million = ?4
           AND output_cost_per_million = ?5
           AND cache_read_cost_per_million = ?6
           AND cache_creation_cost_per_million = ?7
           AND (?8 IS NULL OR display_name = ?8)",
        params![
            model_id,
            SOURCE_BUILTIN,
            version,
            price.input,
            price.output,
            price.cache_read,
            price.cache_creation,
            name
        ],
    )
    .map_err(|e| AppError::Database(format!("标注模型 {model_id} 定价来源失败: {e}")))
}

/// 写入（`overwrite = true` 时覆盖已有行）内置定价的墓碑行。价格列是 NOT NULL，
/// 填目录当前价，读取路径通过 `deleted = 0` 过滤掉墓碑。
pub(crate) fn insert_tombstone(
    conn: &Connection,
    model: &CatalogModel,
    catalog_version: u32,
    now: i64,
    overwrite: bool,
) -> Result<(), AppError> {
    let conflict = if overwrite {
        "ON CONFLICT(model_id) DO UPDATE SET
            source = excluded.source, deleted = 1, updated_at = excluded.updated_at"
    } else {
        "ON CONFLICT(model_id) DO NOTHING"
    };
    let p = &model.price;
    conn.execute(
        &format!(
            "INSERT INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million,
                source, catalog_version, updated_at, deleted
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)
            {conflict}"
        ),
        params![
            model.id,
            model.name,
            p.input,
            p.output,
            p.cache_read,
            p.cache_creation,
            SOURCE_USER,
            catalog_version,
            now
        ],
    )
    .map_err(|e| AppError::Database(format!("写入模型 {} 定价墓碑失败: {e}", model.id)))?;
    Ok(())
}

/// 目录同步：追加缺失的内置行，并把仍为 `builtin`、未删除的行更新到目录当前价。
/// 返回 (新增, 更新) 行数。
pub(crate) fn sync_catalog_rows(
    conn: &Connection,
    catalog: &Catalog,
    now: i64,
) -> Result<(usize, usize), AppError> {
    let mut insert = conn
        .prepare(
            "INSERT OR IGNORE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million,
                source, catalog_version, updated_at, deleted
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)",
        )
        .map_err(|e| AppError::Database(format!("准备模型定价语句失败: {e}")))?;
    let mut update = conn
        .prepare(
            "UPDATE model_pricing SET
                display_name = ?2,
                input_cost_per_million = ?3,
                output_cost_per_million = ?4,
                cache_read_cost_per_million = ?5,
                cache_creation_cost_per_million = ?6,
                catalog_version = ?7,
                updated_at = ?8
             WHERE model_id = ?1
               AND source = 'builtin'
               AND deleted = 0
               AND (display_name IS NOT ?2
                    OR input_cost_per_million IS NOT ?3
                    OR output_cost_per_million IS NOT ?4
                    OR cache_read_cost_per_million IS NOT ?5
                    OR cache_creation_cost_per_million IS NOT ?6
                    OR catalog_version IS NOT ?7)",
        )
        .map_err(|e| AppError::Database(format!("准备模型定价更新语句失败: {e}")))?;

    let (mut inserted, mut updated) = (0usize, 0usize);
    for model in &catalog.models {
        let p = &model.price;
        inserted += insert
            .execute(params![
                model.id,
                model.name,
                p.input,
                p.output,
                p.cache_read,
                p.cache_creation,
                SOURCE_BUILTIN,
                catalog.version,
                now
            ])
            .map_err(|e| AppError::Database(format!("插入模型定价失败: {e}")))?;
        updated += update
            .execute(params![
                model.id,
                model.name,
                p.input,
                p.output,
                p.cache_read,
                p.cache_creation,
                catalog.version,
                now
            ])
            .map_err(|e| AppError::Database(format!("更新模型 {} 定价失败: {e}", model.id)))?;
    }
    Ok((inserted, updated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn exact_and_prefix_lookups_skip_tombstones() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = db.conn.lock().expect("lock conn");
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million, deleted
            ) VALUES ('repo-model-20990101', 'Repo', '1', '2', '0.1', '0.2', 0),
                     ('repo-gone', 'Gone', '9', '9', '9', '9', 1)",
            [],
        )?;

        assert_eq!(find_exact(&conn, "repo-gone")?, None);
        assert_eq!(find_exact(&conn, "repo-model")?, None);
        assert_eq!(
            find_prefix(&conn, "repo-model")?,
            Some(("1".into(), "2".into(), "0.1".into(), "0.2".into()))
        );

        let listed = list_active(&conn)?.expect("table exists");
        assert!(listed.iter().any(|r| r.model_id == "repo-model-20990101"));
        assert!(!listed.iter().any(|r| r.model_id == "repo-gone"));
        Ok(())
    }
}
