//! Cost golden table：同一组 (app_type × input_token_semantics × usage × 定价行)
//! 必须在所有成本路径上算出同一个价格。
//!
//! 覆盖的四条路径：
//! 1. ingest：`CostCalculator::calculate_for_app`（按 app_type 推断语义）
//! 2. logger：`UsageLogger::log_with_calculation` 落库的 semantics 与 total_cost_usd
//! 3. 聚合：`fresh_input_sql` 归一化后的 fresh input 再按定价计费
//! 4. 回填：`Database::backfill_missing_usage_costs` 对零成本行的重算
//!
//! ingest/logger 只会写出 app_type 对应的那一种语义，因此它们只跑语义匹配的行；
//! 聚合与回填会读到历史行，三种语义全部要跑。
//!
//! 已知分歧：回填没有把 grokbuild 视为 cache-inclusive，grokbuild 的 LEGACY/TOTAL
//! 行在回填路径上多计了 cache 部分的输入。这些行被单独放进 #[ignore] 测试，
//! 修复见 docs/roadmap/tickets/P1-trust-release/CCS-092-p1-usage-input-semantics.md。

use super::calculator::{CostCalculator, ModelPricing};
use super::logger::UsageLogger;
use super::parser::TokenUsage;
use crate::app_config::AppType;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::sql_helpers::{
    fresh_input_sql, INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_LEGACY,
    INPUT_TOKEN_SEMANTICS_TOTAL,
};
use rust_decimal::Decimal;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Semantics {
    /// v12 及更早：cache-inclusive app 的 input 只包含 cache read
    Legacy,
    /// cache-inclusive：input 同时包含 cache read 与 cache write
    Total,
    /// cache-exclusive：input 已经是 fresh input
    Fresh,
}

impl Semantics {
    const ALL: [Semantics; 3] = [Semantics::Legacy, Semantics::Total, Semantics::Fresh];

    fn db_value(self) -> i64 {
        match self {
            Semantics::Legacy => INPUT_TOKEN_SEMANTICS_LEGACY,
            Semantics::Total => INPUT_TOKEN_SEMANTICS_TOTAL,
            Semantics::Fresh => INPUT_TOKEN_SEMANTICS_FRESH,
        }
    }
}

/// 每个 app 在 ingest 时写出的语义。穷举 match：新增 AppType 时必须在这里表态。
fn ingest_semantics(app: &AppType) -> Semantics {
    match app {
        AppType::Codex | AppType::Gemini | AppType::GrokBuild => Semantics::Total,
        AppType::Claude
        | AppType::ClaudeDesktop
        | AppType::OpenCode
        | AppType::OpenClaw
        | AppType::Hermes => Semantics::Fresh,
    }
}

struct Scenario {
    name: &'static str,
    model_id: &'static str,
    usage: (u32, u32, u32, u32), // input, output, cache_read, cache_creation
    pricing: (&'static str, &'static str, &'static str, &'static str),
    multiplier: &'static str,
}

const SCENARIOS: [Scenario; 2] = [
    Scenario {
        name: "small",
        model_id: "cost-golden-model-small",
        usage: (1_000, 500, 200, 100),
        pricing: ("3", "15", "0.3", "3.75"),
        multiplier: "1",
    },
    Scenario {
        name: "large-x2",
        model_id: "cost-golden-model-large",
        usage: (2_000_000, 100_000, 500_000, 250_000),
        pricing: ("1.25", "10", "0.125", "1.5"),
        multiplier: "2",
    },
];

/// 期望总价（已含倍率），按 [LEGACY, TOTAL, FRESH] 排列。
///
/// small：output 0.0075 + read 0.00006 + write 0.000375 = 0.007935，
///   fresh input 1000 → 0.003；legacy 800 → 0.0024；total 700 → 0.0021。
/// large-x2：output 1.0 + read 0.0625 + write 0.375 = 1.4375，
///   fresh input 2M → 2.5；legacy 1.5M → 1.875；total 1.25M → 1.5625；再 ×2。
const GOLDEN: &[(&str, &str, [&str; 3])] = &[
    ("claude", "small", ["0.010935", "0.010935", "0.010935"]),
    ("claude", "large-x2", ["7.875", "7.875", "7.875"]),
    (
        "claude-desktop",
        "small",
        ["0.010935", "0.010935", "0.010935"],
    ),
    ("claude-desktop", "large-x2", ["7.875", "7.875", "7.875"]),
    ("codex", "small", ["0.010335", "0.010035", "0.010935"]),
    ("codex", "large-x2", ["6.625", "6.0", "7.875"]),
    ("gemini", "small", ["0.010335", "0.010035", "0.010935"]),
    ("gemini", "large-x2", ["6.625", "6.0", "7.875"]),
    ("grokbuild", "small", ["0.010335", "0.010035", "0.010935"]),
    ("grokbuild", "large-x2", ["6.625", "6.0", "7.875"]),
    ("opencode", "small", ["0.010935", "0.010935", "0.010935"]),
    ("opencode", "large-x2", ["7.875", "7.875", "7.875"]),
    ("openclaw", "small", ["0.010935", "0.010935", "0.010935"]),
    ("openclaw", "large-x2", ["7.875", "7.875", "7.875"]),
    ("hermes", "small", ["0.010935", "0.010935", "0.010935"]),
    ("hermes", "large-x2", ["7.875", "7.875", "7.875"]),
];

#[derive(Debug, Clone)]
struct GoldenRow {
    app: AppType,
    semantics: Semantics,
    scenario: &'static Scenario,
    expected: Decimal,
}

impl GoldenRow {
    fn id(&self) -> String {
        format!(
            "{}/{:?}/{}",
            self.app.as_str(),
            self.semantics,
            self.scenario.name
        )
    }

    fn usage(&self) -> TokenUsage {
        let (input, output, cache_read, cache_creation) = self.scenario.usage;
        TokenUsage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_creation,
            model: None,
            message_id: None,
        }
    }

    fn pricing(&self) -> ModelPricing {
        let (i, o, r, c) = self.scenario.pricing;
        ModelPricing::from_strings(i, o, r, c).unwrap()
    }

    fn multiplier(&self) -> Decimal {
        Decimal::from_str(self.scenario.multiplier).unwrap()
    }

    fn is_ingest_row(&self) -> bool {
        ingest_semantics(&self.app) == self.semantics
    }

    /// 回填路径的已知分歧（CCS-092）。
    fn is_known_backfill_divergence(&self) -> bool {
        self.app == AppType::GrokBuild && self.semantics != Semantics::Fresh
    }
}

impl std::fmt::Debug for Scenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

fn golden_rows() -> Vec<GoldenRow> {
    let mut rows = Vec::new();
    for app in AppType::all() {
        for scenario in SCENARIOS.iter() {
            let (_, _, expected) = GOLDEN
                .iter()
                .find(|(a, s, _)| *a == app.as_str() && *s == scenario.name)
                .unwrap_or_else(|| {
                    panic!("missing golden row for {}/{}", app.as_str(), scenario.name)
                });
            for (idx, semantics) in Semantics::ALL.iter().enumerate() {
                rows.push(GoldenRow {
                    app: app.clone(),
                    semantics: *semantics,
                    scenario,
                    expected: Decimal::from_str(expected[idx]).unwrap(),
                });
            }
        }
    }
    rows
}

fn seed_pricing(db: &Database) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    for scenario in SCENARIOS.iter() {
        let (i, o, r, c) = scenario.pricing;
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
             ) VALUES (?1, ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![scenario.model_id, i, o, r, c],
        )?;
    }
    Ok(())
}

/// 以零成本插入一条历史行，供聚合/回填路径读取。
fn insert_zero_cost_row(db: &Database, row: &GoldenRow) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    let usage = row.usage();
    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, pricing_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_token_semantics,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
            total_cost_usd, latency_ms, status_code, created_at, data_source, cost_multiplier
         ) VALUES (?1, 'provider-golden', ?2, ?3, ?3, ?3, ?4, ?5, ?6, ?7, ?8,
                   '0', '0', '0', '0', '0', 100, 200, 1000, 'proxy', ?9)",
        rusqlite::params![
            row.id(),
            row.app.as_str(),
            row.scenario.model_id,
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_tokens,
            usage.cache_creation_tokens,
            row.semantics.db_value(),
            row.scenario.multiplier,
        ],
    )?;
    Ok(())
}

fn stored_total_cost(db: &Database, request_id: &str) -> Result<Decimal, AppError> {
    let conn = lock_conn!(db.conn);
    let raw: String = conn.query_row(
        "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = ?1",
        [request_id],
        |r| r.get(0),
    )?;
    Ok(Decimal::from_str(&raw).unwrap())
}

/// 路径 1：ingest 计算器。
fn ingest_cost(row: &GoldenRow) -> Decimal {
    CostCalculator::calculate_for_app(
        row.app.as_str(),
        &row.usage(),
        &row.pricing(),
        row.multiplier(),
    )
    .total_cost
}

/// 路径 2：logger 计价并落库，返回 (落库语义, 落库总价)。
fn logger_costs(rows: &[&GoldenRow]) -> Result<Vec<(i64, Decimal)>, AppError> {
    let db = Database::memory()?;
    seed_pricing(&db)?;
    let logger = UsageLogger::new(&db);
    for row in rows {
        logger.log_with_calculation(
            row.id(),
            "provider-golden".to_string(),
            row.app.as_str().to_string(),
            row.scenario.model_id.to_string(),
            row.scenario.model_id.to_string(),
            row.scenario.model_id.to_string(),
            row.usage(),
            row.multiplier(),
            100,
            None,
            200,
            None,
            None,
            false,
        )?;
    }
    rows.iter()
        .map(|row| {
            let semantics: i64 = {
                let conn = lock_conn!(db.conn);
                conn.query_row(
                    "SELECT input_token_semantics FROM proxy_request_logs WHERE request_id = ?1",
                    [row.id()],
                    |r| r.get(0),
                )?
            };
            Ok((semantics, stored_total_cost(&db, &row.id())?))
        })
        .collect()
}

/// 路径 3：聚合层 `fresh_input_sql` 归一化 input，再按定价行计费。
fn aggregation_costs(rows: &[&GoldenRow]) -> Result<Vec<Decimal>, AppError> {
    let db = Database::memory()?;
    for row in rows {
        insert_zero_cost_row(&db, row)?;
    }
    let sql = format!(
        "SELECT {} FROM proxy_request_logs l WHERE l.request_id = ?1",
        fresh_input_sql("l")
    );
    let conn = lock_conn!(db.conn);
    let million = Decimal::from(1_000_000);
    rows.iter()
        .map(|row| {
            let fresh_input: i64 = conn.query_row(&sql, [row.id()], |r| r.get(0))?;
            let usage = row.usage();
            let pricing = row.pricing();
            let base = Decimal::from(fresh_input) * pricing.input_cost_per_million / million
                + Decimal::from(usage.output_tokens) * pricing.output_cost_per_million / million
                + Decimal::from(usage.cache_read_tokens) * pricing.cache_read_cost_per_million
                    / million
                + Decimal::from(usage.cache_creation_tokens)
                    * pricing.cache_creation_cost_per_million
                    / million;
            Ok(base * row.multiplier())
        })
        .collect()
}

/// 路径 4：零成本行回填。
fn backfill_costs(rows: &[&GoldenRow]) -> Result<Vec<Decimal>, AppError> {
    let db = Database::memory()?;
    seed_pricing(&db)?;
    for row in rows {
        insert_zero_cost_row(&db, row)?;
    }
    let updated = db.backfill_missing_usage_costs()?;
    assert_eq!(
        updated as usize,
        rows.len(),
        "every golden row is backfilled"
    );
    rows.iter()
        .map(|row| stored_total_cost(&db, &row.id()))
        .collect()
}

fn check(failures: &mut Vec<String>, path: &str, row: &GoldenRow, actual: Decimal) {
    if actual != row.expected {
        failures.push(format!(
            "[{path}] {}: expected {}, got {actual}",
            row.id(),
            row.expected
        ));
    }
}

#[test]
fn cost_golden_table_covers_every_app_and_semantics() {
    let rows = golden_rows();
    assert_eq!(AppType::all().count(), 8);
    assert_eq!(rows.len(), 8 * Semantics::ALL.len() * SCENARIOS.len());
    for app in AppType::all() {
        for semantics in Semantics::ALL {
            assert!(
                rows.iter()
                    .any(|r| r.app == app && r.semantics == semantics),
                "missing {}/{semantics:?}",
                app.as_str()
            );
        }
    }
    assert_eq!(GOLDEN.len(), 8 * SCENARIOS.len(), "no stale golden rows");
}

#[test]
fn cost_golden_ingest_and_logger_paths() -> Result<(), AppError> {
    let rows = golden_rows();
    let ingest_rows: Vec<&GoldenRow> = rows.iter().filter(|r| r.is_ingest_row()).collect();
    assert_eq!(ingest_rows.len(), 8 * SCENARIOS.len());

    let mut failures = Vec::new();
    for row in &ingest_rows {
        check(&mut failures, "ingest", row, ingest_cost(row));
    }
    for (row, (semantics, cost)) in ingest_rows.iter().zip(logger_costs(&ingest_rows)?) {
        if semantics != row.semantics.db_value() {
            failures.push(format!(
                "[logger] {}: stamped input_token_semantics={semantics}",
                row.id()
            ));
        }
        check(&mut failures, "logger", row, cost);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
fn cost_golden_aggregation_path() -> Result<(), AppError> {
    let rows = golden_rows();
    let all: Vec<&GoldenRow> = rows.iter().collect();
    let mut failures = Vec::new();
    for (row, cost) in all.iter().zip(aggregation_costs(&all)?) {
        check(&mut failures, "fresh_input_sql", row, cost);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
fn cost_golden_backfill_path() -> Result<(), AppError> {
    let rows = golden_rows();
    let selected: Vec<&GoldenRow> = rows
        .iter()
        .filter(|r| !r.is_known_backfill_divergence())
        .collect();
    let mut failures = Vec::new();
    for (row, cost) in selected.iter().zip(backfill_costs(&selected)?) {
        check(&mut failures, "backfill", row, cost);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
#[ignore = "known grokbuild backfill divergence, fixed by CCS-092: \
            docs/roadmap/tickets/P1-trust-release/CCS-092-p1-usage-input-semantics.md"]
fn cost_golden_backfill_path_grokbuild_known_divergence() -> Result<(), AppError> {
    let rows = golden_rows();
    let selected: Vec<&GoldenRow> = rows
        .iter()
        .filter(|r| r.is_known_backfill_divergence())
        .collect();
    assert_eq!(selected.len(), 2 * SCENARIOS.len());
    let mut failures = Vec::new();
    for (row, cost) in selected.iter().zip(backfill_costs(&selected)?) {
        check(&mut failures, "backfill", row, cost);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
