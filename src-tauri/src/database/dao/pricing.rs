//! `model_pricing` data access (DB-15): the pricing table CRUD, the
//! per-row currency/modality metadata and FX rates, and the exact/prefix
//! lookup statements. Model-name normalisation (which candidate ids to try)
//! stays in `services::usage_stats`.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// 模型定价信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingInfo {
    pub model_id: String,
    pub display_name: String,
    pub input_cost_per_million: String,
    pub output_cost_per_million: String,
    pub cache_read_cost_per_million: String,
    pub cache_creation_cost_per_million: String,
    /// PRODUCT-40: currency the prices above are quoted in (costs are
    /// computed in USD; other currencies convert via `pricing_fx_rates`).
    #[serde(default = "default_pricing_currency")]
    pub currency: String,
    /// PROV-33: declared input modalities (`None` = unknown).
    #[serde(default)]
    pub input_modalities: Option<Vec<String>>,
}

fn default_pricing_currency() -> String {
    crate::database::model_catalog::BASE_CURRENCY.to_string()
}

// 模型定价表的读写（原先以裸 SQL 写在 commands/usage.rs 中）。
impl Database {
    /// 列出全部模型定价（按显示名称排序）。表缺失时返回空列表。
    pub fn list_model_pricing(&self) -> Result<Vec<ModelPricingInfo>, AppError> {
        let conn = lock_conn!(self.conn);

        // 检查表是否存在
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='model_pricing'",
                [],
                |row| row.get::<_, i64>(0).map(|count| count > 0),
            )
            .unwrap_or(false);

        if !table_exists {
            log::error!("model_pricing 表不存在,可能需要重启应用以触发数据库迁移");
            return Ok(Vec::new());
        }

        let mut stmt = conn.prepare(
            "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million,
                    currency, input_modalities
             FROM model_pricing
             ORDER BY display_name",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(ModelPricingInfo {
                model_id: row.get(0)?,
                display_name: row.get(1)?,
                input_cost_per_million: row.get(2)?,
                output_cost_per_million: row.get(3)?,
                cache_read_cost_per_million: row.get(4)?,
                cache_creation_cost_per_million: row.get(5)?,
                currency: row.get(6)?,
                input_modalities: crate::database::model_catalog::parse_modalities(
                    row.get::<_, Option<String>>(7)?.as_deref(),
                ),
            })
        })?;

        let mut pricing = Vec::new();
        for row in rows {
            pricing.push(row?);
        }
        Ok(pricing)
    }

    /// 插入或更新一条模型定价（调用方负责校验输入）。
    ///
    /// DB-19: the row becomes `source = 'user'`, so built-in seeding never
    /// overwrites it again.
    pub fn upsert_model_pricing(&self, pricing: &ModelPricingInfo) -> Result<(), AppError> {
        // Prices only: the row's currency and modalities are kept on update
        // (set them with `set_model_pricing_metadata`); a new row starts as
        // USD / unknown modalities.
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million, source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'user')
            ON CONFLICT(model_id) DO UPDATE SET
                display_name = excluded.display_name,
                input_cost_per_million = excluded.input_cost_per_million,
                output_cost_per_million = excluded.output_cost_per_million,
                cache_read_cost_per_million = excluded.cache_read_cost_per_million,
                cache_creation_cost_per_million = excluded.cache_creation_cost_per_million,
                source = 'user'",
            params![
                pricing.model_id,
                pricing.display_name,
                pricing.input_cost_per_million,
                pricing.output_cost_per_million,
                pricing.cache_read_cost_per_million,
                pricing.cache_creation_cost_per_million
            ],
        )
        .map_err(|e| AppError::Database(format!("更新模型定价失败: {e}")))?;
        Ok(())
    }

    /// 删除一条模型定价。
    pub fn delete_model_pricing(&self, model_id: &str) -> Result<(), AppError> {
        {
            let conn = lock_conn!(self.conn);
            conn.execute(
                "DELETE FROM model_pricing WHERE model_id = ?1",
                params![model_id],
            )
            .map_err(|e| AppError::Database(format!("删除模型定价失败: {e}")))?;
        }
        self.refresh_model_catalog();
        Ok(())
    }

    /// Set a pricing row's currency and declared input modalities
    /// (PRODUCT-40, PROV-33). The row becomes the user's (`source = 'user'`),
    /// and the capability snapshot used by the proxy is refreshed at once.
    pub fn set_model_pricing_metadata(
        &self,
        model_id: &str,
        currency: &str,
        input_modalities: Option<&[String]>,
    ) -> Result<(), AppError> {
        let currency = crate::database::model_catalog::normalize_currency(currency)?;
        let modalities = crate::database::model_catalog::modalities_to_json(input_modalities)?;
        let changed = self.write(|conn| {
            conn.execute(
                "UPDATE model_pricing SET currency = ?2, input_modalities = ?3, source = 'user'
                 WHERE model_id = ?1",
                params![model_id.trim(), currency, modalities],
            )
            .map_err(|e| AppError::Database(format!("更新模型定价元数据失败: {e}")))
        })?;
        if changed == 0 {
            return Err(AppError::localized(
                "usage.pricingNotFound",
                format!("模型定价不存在: {model_id}"),
                format!("Model pricing not found: {model_id}"),
            ));
        }
        self.refresh_model_catalog();
        Ok(())
    }

    /// Effective FX rates (units of each currency per 1 USD).
    pub fn get_pricing_fx_rates(
        &self,
    ) -> Result<std::collections::BTreeMap<String, String>, AppError> {
        self.read(|conn| {
            Ok(crate::database::model_catalog::load_fx_rates(conn)?
                .into_iter()
                .map(|(code, rate)| (code, rate.normalize().to_string()))
                .collect())
        })
    }

    /// Store user FX rates (units per 1 USD). Non-positive or unparsable
    /// rates are rejected; `USD` is always 1 and is not stored.
    pub fn set_pricing_fx_rates(
        &self,
        rates: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), AppError> {
        let mut stored = std::collections::BTreeMap::new();
        for (code, rate) in rates {
            let code = crate::database::model_catalog::normalize_currency(code)?;
            let parsed = rust_decimal::Decimal::from_str(rate.trim())
                .ok()
                .filter(|r| *r > rust_decimal::Decimal::ZERO);
            let Some(parsed) = parsed else {
                return Err(AppError::localized(
                    "usage.invalidFxRate",
                    format!("汇率无效: {code} = {rate}"),
                    format!("Invalid exchange rate: {code} = {rate}"),
                ));
            };
            if code != crate::database::model_catalog::BASE_CURRENCY {
                stored.insert(code, parsed.normalize().to_string());
            }
        }
        let json = serde_json::to_string(&stored).map_err(|e| AppError::Config(e.to_string()))?;
        self.set_setting(crate::database::model_catalog::FX_RATES_SETTING, &json)
    }
}

pub(crate) type PricingRowWithCurrency = (String, String, String, String, String);

/// PRODUCT-40: costs are USD. A row quoted in another currency is converted
/// with the configured FX rate; without a rate it is treated as unpriced
/// instead of being summed as dollars.
pub(crate) fn pricing_row_in_usd(
    conn: &Connection,
    (input, output, cache_read, cache_creation, currency): PricingRowWithCurrency,
) -> Result<Option<(String, String, String, String)>, AppError> {
    use crate::database::model_catalog::{load_fx_rates, price_to_usd, BASE_CURRENCY};
    if currency.trim().is_empty() || currency.eq_ignore_ascii_case(BASE_CURRENCY) {
        return Ok(Some((input, output, cache_read, cache_creation)));
    }
    let rates = load_fx_rates(conn)?;
    let convert = |price: &str| price_to_usd(price, currency.trim(), &rates);
    match (
        convert(&input),
        convert(&output),
        convert(&cache_read),
        convert(&cache_creation),
    ) {
        (Some(a), Some(b), Some(c), Some(d)) => Ok(Some((a, b, c, d))),
        _ => {
            log::warn!("模型定价货币 {currency} 没有可用汇率，按未定价处理");
            Ok(None)
        }
    }
}

pub(crate) fn query_model_pricing_exact(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<PricingRowWithCurrency>, AppError> {
    // PERF-31: the usage writer looks prices up for every logged request.
    conn.prepare_cached(
        "SELECT input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million, currency
         FROM model_pricing
         WHERE model_id = ?1",
    )
    .and_then(|mut stmt| {
        stmt.query_row([model_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .optional()
    })
    .map_err(|e| AppError::Database(format!("查询模型定价失败: {e}")))
}

pub(crate) fn query_model_pricing_prefix(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<PricingRowWithCurrency>, AppError> {
    let pattern = format!("{model_id}-%");
    conn.query_row(
        "SELECT input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million, currency
         FROM model_pricing
         WHERE model_id LIKE ?1
         ORDER BY LENGTH(model_id) ASC
         LIMIT 1",
        [pattern],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        },
    )
    .optional()
    .map_err(|e| AppError::Database(format!("查询模型前缀定价失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_pricing_crud_roundtrip() -> Result<(), AppError> {
        let db = Database::memory()?;
        let pricing = ModelPricingInfo {
            model_id: "zz-test-model".to_string(),
            display_name: "ZZ Test".to_string(),
            input_cost_per_million: "1.5".to_string(),
            output_cost_per_million: "2".to_string(),
            cache_read_cost_per_million: "0.1".to_string(),
            cache_creation_cost_per_million: "0.2".to_string(),
            currency: "USD".to_string(),
            input_modalities: None,
        };
        db.upsert_model_pricing(&pricing)?;
        let listed = db.list_model_pricing()?;
        let found = listed
            .iter()
            .find(|p| p.model_id == "zz-test-model")
            .expect("upserted pricing listed");
        assert_eq!(found.input_cost_per_million, "1.5");
        assert_eq!(found.display_name, "ZZ Test");

        db.upsert_model_pricing(&ModelPricingInfo {
            output_cost_per_million: "3".to_string(),
            ..pricing
        })?;
        let replaced = db.list_model_pricing()?;
        assert_eq!(
            replaced
                .iter()
                .filter(|p| p.model_id == "zz-test-model")
                .count(),
            1
        );

        db.delete_model_pricing("zz-test-model")?;
        assert!(db
            .list_model_pricing()?
            .iter()
            .all(|p| p.model_id != "zz-test-model"));
        Ok(())
    }
}
