//! 内置模型定价目录
//!
//! 价格以数据（`catalog.json`）发布，不再写在 Rust 代码里：
//! - `models[]` 是内置定价行，启动时 `INSERT OR IGNORE`，不覆盖已有行；
//!   用户删除过的内置定价（settings 中的 `deleted_builtin_model_pricing`）会被跳过。
//! - `supersedes[]` 列出该模型此前发布过的旧价。表里的行仍等于某个旧价时，
//!   说明用户没有改过，启动时更新到当前价；用户改过的行保持不动。
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

/// 插入目录中的默认模型定价（INSERT OR IGNORE，不覆盖已有行），跳过用户删除过的内置定价。
pub fn seed(conn: &Connection) -> Result<(), AppError> {
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

/// 把仍等于目录旧价（`supersedes`）的行更新到当前价；用户改过的行不动。
pub fn apply_supersedes(conn: &Connection) -> Result<(), AppError> {
    for model in &catalog().models {
        let p = &model.price;
        for old in &model.supersedes {
            conn.execute(
                "UPDATE model_pricing SET
                    display_name = ?2,
                    input_cost_per_million = ?3,
                    output_cost_per_million = ?4,
                    cache_read_cost_per_million = ?5,
                    cache_creation_cost_per_million = ?6
                 WHERE model_id = ?1
                   AND input_cost_per_million = ?7
                   AND output_cost_per_million = ?8
                   AND cache_read_cost_per_million = ?9
                   AND cache_creation_cost_per_million = ?10",
                params![
                    model.id,
                    model.name,
                    p.input,
                    p.output,
                    p.cache_read,
                    p.cache_creation,
                    old.input,
                    old.output,
                    old.cache_read,
                    old.cache_creation
                ],
            )
            .map_err(|e| AppError::Database(format!("修复模型 {} 定价失败: {e}", model.id)))?;
        }
    }
    Ok(())
}

/// 启动 / 恢复数据库时同步目录：增量追加新模型，并修复仍等于旧内置值的定价。
pub fn sync(conn: &Connection) -> Result<(), AppError> {
    seed(conn)?;
    apply_supersedes(conn)
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

    #[test]
    fn pricing_catalog_sync_repairs_only_untouched_rows() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE model_pricing (
                model_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
                input_cost_per_million TEXT NOT NULL, output_cost_per_million TEXT NOT NULL,
                cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
                cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
            );
            INSERT INTO model_pricing VALUES ('gpt-5.6-sol', 'old', '5', '30', '0.50', '0');
            INSERT INTO model_pricing VALUES ('gpt-5.6-luna', 'mine', '9', '9', '9', '9');",
        )?;

        sync(&conn)?;

        let row = |id: &str| -> Result<(String, String), AppError> {
            Ok(conn.query_row(
                "SELECT display_name, cache_creation_cost_per_million
                 FROM model_pricing WHERE model_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        };
        assert_eq!(row("gpt-5.6-sol")?, ("GPT-5.6 Sol".into(), "6.25".into()));
        assert_eq!(row("gpt-5.6-luna")?, ("mine".into(), "9".into()));
        assert_eq!(row("claude-opus-4-8")?.0, "Claude Opus 4.8");
        Ok(())
    }
}
