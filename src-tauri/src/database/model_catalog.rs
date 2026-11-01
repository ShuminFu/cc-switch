//! Per-model catalog metadata stored on `model_pricing` rows (PROV-33, PRODUCT-40).
//!
//! - `input_modalities` (v24): JSON array such as `["text"]` or
//!   `["text","image"]`; `NULL` means unknown. The proxy's media sanitizer and
//!   the Codex catalog read it through [`declared_input_modalities`], so
//!   marking a model text-only (or image-capable) in the pricing table changes
//!   behaviour without a rebuild.
//! - `currency` (v25): the currency the row's prices are quoted in (`USD`
//!   default). Costs are always computed and stored in USD: a non-USD row is
//!   converted with the FX rate from the `pricing_fx_rates` setting (units of
//!   the currency per 1 USD) when the price is looked up.
//!
//! The proxy hot path has no database handle, so the app database publishes
//! the declared modalities into a process-wide snapshot that is refreshed
//! after every write to `model_pricing` made through `Database`.

use crate::error::AppError;
use rusqlite::{params, Connection, OptionalExtension};
use rust_decimal::Decimal;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use std::sync::RwLock;

/// Model tails confirmed to accept text input only. Seeds
/// `model_pricing.input_modalities` (v24 and fresh databases) and remains the
/// fallback for models that have no catalog row / no declared modalities.
///
/// Deliberately exact and fail-open: a new suffix is not inherited
/// automatically (a future `-vision`/`-vl` variant stays image-capable).
pub(crate) const CONFIRMED_TEXT_ONLY_TAILS: &[&str] = &[
    "ark-code-latest",
    "deepseek-chat",
    "deepseek-reasoner",
    "deepseek-v4-flash",
    "deepseek-v4-pro",
    "glm-5.1",
    // Exact rather than prefix matching: GLM visual models use a `v`
    // suffix (for example glm-5.2v), which must remain image-capable.
    "glm-5.2",
    "kat-coder",
    "kat-coder-pro",
    "kat-coder-pro v1",
    "kat-coder-pro v2",
    "kat-coder-pro-v1",
    "kat-coder-pro-v2",
    "ling-2.5-1t",
    "longcat-2.0",
    "longcat-flash-chat",
    "minimax-m2.7",
    "minimax-m2.7-highspeed",
    "mimo-v2.5-pro",
    "qwen3-coder-480b",
    "qwen3-coder-480b-a35b-instruct",
    "qwen3-coder-flash",
    "qwen3-coder-next",
    "qwen3-coder-plus",
    "step-3.5-flash",
    "step-3.5-flash-2603",
    "us.deepseek.r1-v1",
];

/// JSON stored for a text-only model.
pub(crate) const TEXT_ONLY_MODALITIES_JSON: &str = r#"["text"]"#;

/// Settings key holding `{ "<CURRENCY>": "<units per 1 USD>" }`.
pub(crate) const FX_RATES_SETTING: &str = "pricing_fx_rates";

/// The currency costs are computed and stored in.
pub(crate) const BASE_CURRENCY: &str = "USD";

/// Built-in fallback rates used when the user has not set one (units per USD).
const DEFAULT_FX_RATES: &[(&str, &str)] = &[("CNY", "7.2")];

static DECLARED_MODALITIES: RwLock<Option<HashMap<String, Vec<String>>>> = RwLock::new(None);

fn db_err(context: &'static str) -> impl Fn(rusqlite::Error) -> AppError {
    move |e| AppError::Database(format!("{context}: {e}"))
}

/// Set `input_modalities = ["text"]` on the confirmed text-only rows that do
/// not declare modalities yet. Built-in rows only, so a user's own row is
/// never changed behind their back.
pub(crate) fn seed_text_only_modalities(conn: &Connection) -> Result<usize, AppError> {
    let mut stmt = conn
        .prepare(
            "UPDATE model_pricing SET input_modalities = ?1
             WHERE model_id = ?2 AND input_modalities IS NULL AND source = 'builtin'",
        )
        .map_err(db_err("准备模型输入模态语句失败"))?;
    let mut changed = 0;
    for tail in CONFIRMED_TEXT_ONLY_TAILS {
        changed += stmt
            .execute(params![TEXT_ONLY_MODALITIES_JSON, tail])
            .map_err(db_err("写入模型输入模态失败"))?;
    }
    Ok(changed)
}

/// Parse a stored modality list; malformed JSON counts as unknown.
pub(crate) fn parse_modalities(raw: Option<&str>) -> Option<Vec<String>> {
    let items: Vec<String> = serde_json::from_str(raw?).ok()?;
    let items: Vec<String> = items
        .into_iter()
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .collect();
    (!items.is_empty()).then_some(items)
}

/// Normalise a user-supplied modality list for storage (`None` = unknown).
pub(crate) fn modalities_to_json(items: Option<&[String]>) -> Result<Option<String>, AppError> {
    let Some(items) = items else {
        return Ok(None);
    };
    let mut normalized: Vec<String> = Vec::new();
    for item in items {
        let item = item.trim().to_ascii_lowercase();
        if item.is_empty() {
            continue;
        }
        if !matches!(item.as_str(), "text" | "image" | "audio" | "video" | "pdf") {
            return Err(AppError::localized(
                "usage.invalidModality",
                format!("未知的输入模态: {item}"),
                format!("Unknown input modality: {item}"),
            ));
        }
        if !normalized.contains(&item) {
            normalized.push(item);
        }
    }
    if normalized.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(&normalized)
        .map(Some)
        .map_err(|e| AppError::Config(e.to_string()))
}

/// Load `model_id → modalities` for every row that declares them.
pub(crate) fn load_declared_modalities(
    conn: &Connection,
) -> Result<HashMap<String, Vec<String>>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT model_id, input_modalities FROM model_pricing
             WHERE input_modalities IS NOT NULL",
        )
        .map_err(db_err("读取模型输入模态失败"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(db_err("读取模型输入模态失败"))?;
    let mut map = HashMap::new();
    for row in rows {
        let (model_id, raw) = row.map_err(db_err("读取模型输入模态失败"))?;
        if let Some(items) = parse_modalities(raw.as_deref()) {
            map.insert(model_id.trim().to_ascii_lowercase(), items);
        }
    }
    Ok(map)
}

/// Replace the process-wide snapshot read by [`declared_input_modalities`].
pub(crate) fn publish_declared_modalities(map: HashMap<String, Vec<String>>) {
    match DECLARED_MODALITIES.write() {
        Ok(mut guard) => *guard = Some(map),
        Err(poisoned) => *poisoned.into_inner() = Some(map),
    }
}

/// The modalities the catalog declares for `model_id` (already normalised:
/// lower-case, no `models/` prefix). Tries the full id first, then the part
/// after the last `/`. `None` when the catalog has no declaration (or none
/// has been published yet); callers then fall back to
/// [`CONFIRMED_TEXT_ONLY_TAILS`].
pub(crate) fn declared_input_modalities(model_id: &str) -> Option<Vec<String>> {
    let guard = match DECLARED_MODALITIES.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let map = guard.as_ref()?;
    if let Some(items) = map.get(model_id) {
        return Some(items.clone());
    }
    let tail = model_id.rsplit('/').next().unwrap_or(model_id);
    map.get(tail).cloned()
}

/// Normalise a currency code (`" cny "` → `"CNY"`); empty means USD.
pub(crate) fn normalize_currency(raw: &str) -> Result<String, AppError> {
    let code = raw.trim().to_ascii_uppercase();
    if code.is_empty() {
        return Ok(BASE_CURRENCY.to_string());
    }
    if code.len() != 3 || !code.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(AppError::localized(
            "usage.invalidCurrency",
            format!("无效的货币代码: {raw}"),
            format!("Invalid currency code: {raw}"),
        ));
    }
    Ok(code)
}

/// Effective FX rates (units per 1 USD): built-in defaults overlaid with the
/// user's `pricing_fx_rates` setting. Invalid entries are ignored.
pub(crate) fn load_fx_rates(conn: &Connection) -> Result<BTreeMap<String, Decimal>, AppError> {
    let mut rates: BTreeMap<String, Decimal> = DEFAULT_FX_RATES
        .iter()
        .filter_map(|(code, rate)| Some(((*code).to_string(), Decimal::from_str(rate).ok()?)))
        .collect();
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [FX_RATES_SETTING],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_err("读取汇率设置失败"))?;
    if let Some(raw) = raw {
        match serde_json::from_str::<BTreeMap<String, serde_json::Value>>(&raw) {
            Ok(map) => {
                for (code, value) in map {
                    let rate = match &value {
                        serde_json::Value::String(s) => Decimal::from_str(s.trim()).ok(),
                        serde_json::Value::Number(n) => Decimal::from_str(&n.to_string()).ok(),
                        _ => None,
                    };
                    if let (Ok(code), Some(rate)) = (normalize_currency(&code), rate) {
                        if rate > Decimal::ZERO {
                            rates.insert(code, rate);
                        }
                    }
                }
            }
            Err(e) => log::warn!("pricing_fx_rates 设置无法解析，使用默认汇率: {e}"),
        }
    }
    rates.insert(BASE_CURRENCY.to_string(), Decimal::ONE);
    Ok(rates)
}

/// Convert a per-million price quoted in `currency` into USD. `None` when the
/// currency has no FX rate (the row is then treated as unpriced rather than
/// silently summed as dollars).
pub(crate) fn price_to_usd(
    price: &str,
    currency: &str,
    rates: &BTreeMap<String, Decimal>,
) -> Option<String> {
    if currency.eq_ignore_ascii_case(BASE_CURRENCY) {
        return Some(price.to_string());
    }
    let rate = rates.get(&currency.to_ascii_uppercase())?;
    let value = Decimal::from_str(price.trim()).ok()?;
    Some((value / rate).round_dp(12).normalize().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modalities_parse_and_normalise() {
        assert_eq!(
            parse_modalities(Some(r#"[" Text ","IMAGE"]"#)),
            Some(vec!["text".to_string(), "image".to_string()])
        );
        assert_eq!(parse_modalities(Some("[]")), None);
        assert_eq!(parse_modalities(Some("not json")), None);
        assert_eq!(parse_modalities(None), None);
        assert_eq!(
            modalities_to_json(Some(&["TEXT".into(), "text".into(), "image".into()])).unwrap(),
            Some(r#"["text","image"]"#.to_string())
        );
        assert!(modalities_to_json(Some(&["smell".into()])).is_err());
        assert_eq!(modalities_to_json(Some(&[])).unwrap(), None);
    }

    #[test]
    fn fx_conversion_uses_settings_over_defaults() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        conn.execute(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT)",
            [],
        )?;
        let rates = load_fx_rates(&conn)?;
        assert_eq!(rates.get("CNY"), Some(&Decimal::from_str("7.2").unwrap()));
        // PRODUCT-40 acceptance: ¥1/M with FX 7.2 is $0.139/M.
        let usd = price_to_usd("1", "CNY", &rates).unwrap();
        assert!(usd.starts_with("0.13888"), "{usd}");
        assert_eq!(price_to_usd("3", "usd", &rates).as_deref(), Some("3"));
        assert_eq!(price_to_usd("3", "EUR", &rates), None);

        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)",
            params![FX_RATES_SETTING, r#"{"cny":"8","EUR":0.5,"bad":"x"}"#],
        )?;
        let rates = load_fx_rates(&conn)?;
        assert_eq!(price_to_usd("8", "CNY", &rates).as_deref(), Some("1"));
        assert_eq!(price_to_usd("1", "EUR", &rates).as_deref(), Some("2"));
        Ok(())
    }

    fn stored_modalities(conn: &Connection, model_id: &str) -> Option<String> {
        conn.query_row(
            "SELECT input_modalities FROM model_pricing WHERE model_id = ?1",
            [model_id],
            |row| row.get(0),
        )
        .unwrap()
    }

    /// PROV-33: a fresh database seeds the confirmed text-only rows, and the
    /// v23→v24/v25 upgrade adds both columns, seeds built-in rows only and
    /// leaves a user's own row alone.
    #[test]
    fn migrations_add_catalog_columns_and_seed_text_only_rows() -> Result<(), AppError> {
        let db = crate::database::Database::memory()?;
        let conn = db.conn.lock().unwrap();
        assert_eq!(
            stored_modalities(&conn, "glm-5.1").as_deref(),
            Some(TEXT_ONLY_MODALITIES_JSON)
        );
        let currency: String = conn.query_row(
            "SELECT currency FROM model_pricing WHERE model_id = 'glm-5.1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(currency, "USD");

        // Roll the table back to its v23 shape.
        conn.execute_batch(
            "ALTER TABLE model_pricing DROP COLUMN input_modalities;
             ALTER TABLE model_pricing DROP COLUMN currency;
             UPDATE model_pricing SET source = 'user' WHERE model_id = 'deepseek-chat';
             PRAGMA user_version = 23;",
        )?;
        crate::database::Database::apply_schema_migrations_on_conn(&conn)?;
        assert_eq!(
            crate::database::Database::get_user_version(&conn)?,
            crate::database::SCHEMA_VERSION
        );
        assert_eq!(
            stored_modalities(&conn, "glm-5.1").as_deref(),
            Some(TEXT_ONLY_MODALITIES_JSON)
        );
        assert_eq!(stored_modalities(&conn, "deepseek-chat"), None);
        assert_eq!(stored_modalities(&conn, "claude-sonnet-5"), None);
        let currency: String = conn.query_row(
            "SELECT currency FROM model_pricing WHERE model_id = 'claude-sonnet-5'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(currency, "USD");
        Ok(())
    }

    /// PROV-33 acceptance: editing a row's modalities changes the proxy's
    /// text-only decision without a rebuild, and a declaration overrides the
    /// static seed list.
    #[test]
    fn catalog_edits_drive_text_only_detection() -> Result<(), AppError> {
        use crate::model_capabilities::is_confirmed_text_only_model;
        let mut db = crate::database::Database::memory()?;
        db.publishes_model_catalog = true;
        let model = "zz-catalog-probe-model";
        assert!(!is_confirmed_text_only_model(model));
        db.upsert_model_pricing(&crate::services::usage_stats::ModelPricingInfo {
            model_id: model.to_string(),
            display_name: "Probe".to_string(),
            input_cost_per_million: "1".to_string(),
            output_cost_per_million: "1".to_string(),
            cache_read_cost_per_million: "0".to_string(),
            cache_creation_cost_per_million: "0".to_string(),
            currency: "USD".to_string(),
            input_modalities: None,
        })?;
        db.set_model_pricing_metadata(model, "USD", Some(&["text".to_string()]))?;
        assert!(is_confirmed_text_only_model(model));
        assert!(is_confirmed_text_only_model(&format!("vendor/{model}")));
        assert!(is_confirmed_text_only_model("glm-5.1"));

        db.set_model_pricing_metadata(model, "USD", Some(&["text".into(), "image".into()]))?;
        assert!(!is_confirmed_text_only_model(model));

        db.set_model_pricing_metadata(model, "USD", None)?;
        assert!(!is_confirmed_text_only_model(model));
        db.delete_model_pricing(model)?;
        assert!(!is_confirmed_text_only_model(model));
        // Models without a catalog declaration keep the seed-list fallback.
        assert!(is_confirmed_text_only_model("kat-coder-pro"));
        Ok(())
    }

    #[test]
    fn currency_codes_are_validated() {
        assert_eq!(normalize_currency(" cny ").unwrap(), "CNY");
        assert_eq!(normalize_currency("").unwrap(), "USD");
        assert!(normalize_currency("dollars").is_err());
    }
}
