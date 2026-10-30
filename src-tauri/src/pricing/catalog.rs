//! 内置模型定价目录
//!
//! 价格以数据（`catalog.json`）发布，不再写在 Rust 代码里：
//! - `models[]` 是内置定价行，启动时 `INSERT OR IGNORE`，不覆盖已有行。
//! - `supersedes[]` 列出该模型此前发布过的旧价，供 v18 迁移判断旧行是否被用户改过。
//!
//! 每行定价带来源（provenance，CCS-158）：
//! - `source = 'builtin'`：仍等于某个目录价的内置行，启动时跟随目录更新到当前价，
//!   并记录 `catalog_version`；
//! - `source = 'user'`：用户新增或修改过的行，目录更新不再覆盖；
//! - `deleted = 1`：用户删除过的内置定价（墓碑），目录更新与重启都不会把它插回来。
//!   P1 存在 settings `deleted_builtin_model_pricing` 中的删除标记在启动时并入墓碑，
//!   该标记仍会双写，供只认识它的旧版本使用。
//!
//! 调价 = 改 `models[]` 里的价格，并把旧价追加到 `supersedes[]`。

use crate::error::AppError;
use rusqlite::{params, Connection};
use serde::Deserialize;
use std::sync::LazyLock;

const CATALOG_JSON: &str = include_str!("catalog.json");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub models: Vec<CatalogModel>,
}

/// 每百万 token 单价（USD 字符串，保持与 `model_pricing` 表一致的 TEXT 存储）
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPrice {
    pub input: String,
    pub output: String,
    pub cache_read: String,
    pub cache_creation: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogModel {
    /// 使用短横线格式（如 claude-haiku-4-5），与 API 返回的模型名称标准化后一致
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub price: CatalogPrice,
    #[serde(default)]
    pub supersedes: Vec<CatalogPrice>,
    /// 仅供维护者阅读的定价说明
    #[serde(default)]
    #[allow(dead_code)]
    pub note: Option<String>,
    /// 仅供维护者阅读的调价说明
    #[serde(default)]
    #[allow(dead_code)]
    pub supersedes_note: Option<String>,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(CATALOG_JSON).expect("内置定价目录 pricing/catalog.json 格式错误")
});

/// 内置定价目录（编译期嵌入，首次访问时解析）
pub fn catalog() -> &'static Catalog {
    &CATALOG
}

/// 是否为内置（目录内）模型定价
pub fn is_builtin(model_id: &str) -> bool {
    catalog().models.iter().any(|m| m.id == model_id)
}

/// 内置行（跟随目录更新）
pub const SOURCE_BUILTIN: &str = "builtin";
/// 用户新增或修改过的行（目录更新不覆盖）
pub const SOURCE_USER: &str = "user";

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

/// v18 之前的迁移步骤（v2 / v9）使用的 seed：只写基础列（那时还没有来源列），
/// 跳过 settings 中记录的已删除内置定价。已发布的迁移步骤行为保持不变。
pub fn legacy_seed(conn: &Connection) -> Result<(), AppError> {
    let deleted = crate::settings::deleted_builtin_model_pricing_ids();
    let mut stmt = conn
        .prepare(
            "INSERT OR IGNORE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .map_err(|e| AppError::Database(format!("准备模型定价语句失败: {e}")))?;

    let mut count = 0usize;
    for model in &catalog().models {
        if deleted.iter().any(|id| id == &model.id) {
            continue;
        }
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

    log::info!(
        "已插入 {count} 条默认模型定价数据（目录 v{}）",
        catalog().version
    );
    Ok(())
}

/// v18 迁移：给已有行标注来源。价格仍等于目录当前价或某个旧价（`supersedes`）的内置行
/// 视为未改过（`builtin`），其余行（自定义模型、改过价格的内置行）保持列默认值 `user`。
///
/// 只比较四个价格、不比较显示名：目录没有记录历史显示名，按显示名判断会把未改过的
/// 旧行误判为用户行，此后再也收不到调价。
pub fn classify_provenance(conn: &Connection) -> Result<(), AppError> {
    let catalog = catalog();
    let mark = |model_id: &str, price: &CatalogPrice, version: Option<u32>| {
        conn.execute(
            "UPDATE model_pricing SET source = ?2, catalog_version = ?3
             WHERE model_id = ?1
               AND input_cost_per_million = ?4
               AND output_cost_per_million = ?5
               AND cache_read_cost_per_million = ?6
               AND cache_creation_cost_per_million = ?7",
            params![
                model_id,
                SOURCE_BUILTIN,
                version,
                price.input,
                price.output,
                price.cache_read,
                price.cache_creation
            ],
        )
        .map_err(|e| AppError::Database(format!("标注模型 {model_id} 定价来源失败: {e}")))
    };

    let mut builtin = 0usize;
    for model in &catalog.models {
        builtin += mark(&model.id, &model.price, Some(catalog.version))?;
        // 旧价行不记录目录版本（来自目录之前的 seed），启动同步时更新到当前价
        for old in &model.supersedes {
            builtin += mark(&model.id, old, None)?;
        }
    }
    log::info!("模型定价来源标注完成：{builtin} 条内置行，其余为用户行");
    Ok(())
}

/// 把 P1 存在 settings 中的删除标记并入墓碑：只为当前不存在的内置行写墓碑。
/// 行仍存在时（例如同步来的快照里还有这一行）保持原样，与 P1 的 seed 行为一致。
fn fold_legacy_deletions(
    conn: &Connection,
    catalog: &Catalog,
    legacy_deleted: &[String],
) -> Result<(), AppError> {
    for model_id in legacy_deleted {
        if let Some(model) = catalog.models.iter().find(|m| &m.id == model_id) {
            insert_tombstone(conn, catalog, model, false)?;
        }
    }
    Ok(())
}

/// 写入（`overwrite = true` 时覆盖已有行）内置定价的墓碑行。价格列是 NOT NULL，
/// 填目录当前价，读取路径通过 `deleted = 0` 过滤掉墓碑。
fn insert_tombstone(
    conn: &Connection,
    catalog: &Catalog,
    model: &CatalogModel,
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
            catalog.version,
            now_ts()
        ],
    )
    .map_err(|e| AppError::Database(format!("写入模型 {} 定价墓碑失败: {e}", model.id)))?;
    Ok(())
}

/// 删除内置定价：写墓碑（保留行并标记 `deleted = 1`），之后的目录同步与重启都不会恢复它。
/// 不是内置定价时返回 `false`，由调用方直接删除该行。
pub fn tombstone(conn: &Connection, model_id: &str) -> Result<bool, AppError> {
    let catalog = catalog();
    let Some(model) = catalog.models.iter().find(|m| m.id == model_id) else {
        return Ok(false);
    };
    insert_tombstone(conn, catalog, model, true)?;
    Ok(true)
}

/// 启动 / 恢复数据库时同步目录：先把 settings 中的删除标记并入墓碑，再增量追加新模型，
/// 并把仍为 `builtin` 的行更新到当前目录价。用户行与墓碑不动。
pub fn sync(conn: &Connection) -> Result<(), AppError> {
    sync_with(
        conn,
        catalog(),
        &crate::settings::deleted_builtin_model_pricing_ids(),
    )
}

pub(crate) fn sync_with(
    conn: &Connection,
    catalog: &Catalog,
    legacy_deleted: &[String],
) -> Result<(), AppError> {
    fold_legacy_deletions(conn, catalog, legacy_deleted)?;

    let now = now_ts();
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

    log::info!(
        "模型定价目录 v{} 同步完成：新增 {inserted} 条，更新 {updated} 条内置行",
        catalog.version
    );
    Ok(())
}

/// 测试用：内置目录的下一个版本，给指定模型换一个新价格与显示名
#[cfg(test)]
pub(crate) fn next_catalog_for_test(repriced: &[&str]) -> Catalog {
    let mut next: Catalog = serde_json::from_str(CATALOG_JSON).expect("parse catalog");
    next.version += 1;
    for model in next.models.iter_mut() {
        if repriced.contains(&model.id.as_str()) {
            model.price.input = "123".to_string();
            model.name = format!("{} (v{})", model.name, next.version);
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::collections::HashSet;
    use std::str::FromStr;

    fn assert_price(id: &str, p: &CatalogPrice) {
        for (field, v) in [
            ("input", &p.input),
            ("output", &p.output),
            ("cache_read", &p.cache_read),
            ("cache_creation", &p.cache_creation),
        ] {
            let d = Decimal::from_str(v)
                .unwrap_or_else(|e| panic!("{id}.{field} = {v:?} 不是合法数字: {e}"));
            assert!(d >= Decimal::ZERO, "{id}.{field} 为负数");
        }
    }

    #[test]
    fn pricing_catalog_parses_and_is_well_formed() {
        let catalog = catalog();
        assert!(catalog.version >= 1);
        assert!(catalog.models.len() >= 170, "目录不应意外丢行");

        let mut ids = HashSet::new();
        for model in &catalog.models {
            assert!(ids.insert(model.id.as_str()), "重复的模型 id: {}", model.id);
            assert_eq!(
                model.id,
                model.id.trim().to_ascii_lowercase(),
                "模型 id 必须是归一化后的小写形式"
            );
            assert!(!model.name.trim().is_empty(), "{} 缺少显示名", model.id);
            assert_price(&model.id, &model.price);
            for old in &model.supersedes {
                assert_price(&model.id, old);
                assert_ne!(old, &model.price, "{} 的旧价不能等于当前价", model.id);
            }
        }
    }

    #[test]
    fn pricing_catalog_keeps_known_rows() {
        let find = |id: &str| catalog().models.iter().find(|m| m.id == id).unwrap();
        let sonnet = find("claude-sonnet-4-5-20250929");
        assert_eq!(sonnet.price.input, "3");
        assert_eq!(sonnet.price.cache_creation, "3.75");
        let sol = find("gpt-5.6-sol");
        assert_eq!(sol.price.cache_creation, "6.25");
        assert_eq!(sol.supersedes.len(), 1);
        assert_eq!(sol.supersedes[0].cache_creation, "0");
        assert!(is_builtin("gpt-5.6-sol"));
        assert!(!is_builtin("my-custom-model"));
    }

    /// (source, deleted, catalog_version, input, display_name)
    type ProvenanceRow = (String, i64, Option<u32>, String, String);

    fn provenance(conn: &Connection, id: &str) -> Result<ProvenanceRow, AppError> {
        Ok(conn.query_row(
            "SELECT source, deleted, catalog_version, input_cost_per_million, display_name
             FROM model_pricing WHERE model_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?)
    }

    #[test]
    fn pricing_catalog_update_keeps_user_rows_and_tombstones() -> Result<(), AppError> {
        let home = tempfile::tempdir().expect("tempdir");
        let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
        let _settings = crate::settings::scoped_settings_store();
        let db = crate::database::Database::memory()?;
        db.save_user_model_pricing(
            "claude-opus-4-8",
            "My Opus",
            CatalogPrice {
                input: "4".into(),
                output: "20".into(),
                cache_read: "0.4".into(),
                cache_creation: "5".into(),
            },
        )?;
        db.delete_model_pricing("claude-sonnet-5")?;
        let conn = crate::database::lock_conn!(db.conn);
        let v = catalog().version;
        assert_eq!(
            provenance(&conn, "claude-haiku-4-5-20251001")?,
            (
                "builtin".into(),
                0,
                Some(v),
                "1".into(),
                "Claude Haiku 4.5".into()
            )
        );

        let next = next_catalog_for_test(&[
            "claude-opus-4-8",
            "claude-sonnet-5",
            "claude-haiku-4-5-20251001",
        ]);
        sync_with(&conn, &next, &[])?;

        // 未改过的内置行跟随目录更新
        assert_eq!(
            provenance(&conn, "claude-haiku-4-5-20251001")?,
            (
                "builtin".into(),
                0,
                Some(v + 1),
                "123".into(),
                format!("Claude Haiku 4.5 (v{})", v + 1)
            )
        );
        // 其余内置行只更新目录版本
        assert_eq!(provenance(&conn, "gpt-5.5")?.2, Some(v + 1));
        // 用户编辑过的行不被覆盖
        assert_eq!(
            provenance(&conn, "claude-opus-4-8")?,
            ("user".into(), 0, None, "4".into(), "My Opus".into())
        );
        // 删除过的内置行保持删除，查价也查不到
        let (source, deleted, ..) = provenance(&conn, "claude-sonnet-5")?;
        assert_eq!((source.as_str(), deleted), ("user", 1));
        assert!(
            crate::pricing::resolver::find_model_pricing_row(&conn, "claude-sonnet-5")?.is_none()
        );
        Ok(())
    }

    #[test]
    fn pricing_legacy_settings_deletions_become_tombstones() -> Result<(), AppError> {
        let db = crate::database::Database::memory()?;
        let conn = crate::database::lock_conn!(db.conn);
        // P1 的删除：行已不在表中，只在 settings 里记了删除标记
        conn.execute(
            "DELETE FROM model_pricing WHERE model_id = 'gpt-5.4-mini'",
            [],
        )?;
        let legacy = vec![
            "gpt-5.4-mini".to_string(),
            // 行仍存在（例如同步来的快照里还有）时保持原样，与 P1 行为一致
            "gpt-5.4-nano".to_string(),
            // 不是内置定价的 id 被忽略
            "not-a-builtin".to_string(),
        ];

        sync_with(&conn, catalog(), &legacy)?;
        sync_with(&conn, catalog(), &legacy)?;

        let (source, deleted, ..) = provenance(&conn, "gpt-5.4-mini")?;
        assert_eq!((source.as_str(), deleted), ("user", 1));
        assert!(crate::pricing::resolver::find_model_pricing_row(&conn, "gpt-5.4-mini")?.is_none());
        assert_eq!(provenance(&conn, "gpt-5.4-nano")?.1, 0);
        let custom: i64 = conn.query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE model_id = 'not-a-builtin'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(custom, 0);

        // 之后的目录更新也不会恢复它
        sync_with(&conn, &next_catalog_for_test(&["gpt-5.4-mini"]), &[])?;
        assert_eq!(provenance(&conn, "gpt-5.4-mini")?.1, 1);
        Ok(())
    }
}
