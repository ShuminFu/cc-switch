//! 成本引擎：全项目唯一的成本计算函数 [`cost`]
//!
//! 代理 ingest（`CostCalculator`）、会话日志导入和零成本行回填都调用这里，
//! 不允许在别处再写一份 `tokens × 单价 / 1M` 的算式。使用高精度 Decimal
//! 避免浮点数精度问题。

use crate::app_config::InputSemantics;
use crate::services::sql_helpers::{INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_TOTAL};
use rust_decimal::Decimal;
use std::str::FromStr;

/// 成本明细
#[derive(Debug, Clone)]
pub struct CostBreakdown {
    pub input_cost: Decimal,
    pub output_cost: Decimal,
    pub cache_read_cost: Decimal,
    pub cache_creation_cost: Decimal,
    pub total_cost: Decimal,
}

/// 模型定价信息（每百万 token 单价）
#[derive(Debug, Clone)]
pub struct ModelPricing {
    pub input_cost_per_million: Decimal,
    pub output_cost_per_million: Decimal,
    pub cache_read_cost_per_million: Decimal,
    pub cache_creation_cost_per_million: Decimal,
}

impl ModelPricing {
    /// 从字符串创建定价信息
    pub fn from_strings(
        input: &str,
        output: &str,
        cache_read: &str,
        cache_creation: &str,
    ) -> Result<Self, rust_decimal::Error> {
        Ok(Self {
            input_cost_per_million: Decimal::from_str(input)?,
            output_cost_per_million: Decimal::from_str(output)?,
            cache_read_cost_per_million: Decimal::from_str(cache_read)?,
            cache_creation_cost_per_million: Decimal::from_str(cache_creation)?,
        })
    }
}

/// 一次请求的 token 计数（与来源无关）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

/// `input` 计数里包含了哪些 cache 部分，决定计费前要扣除什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputBasis {
    /// input 已经是 fresh input（Anthropic 风格），不能再次扣减。
    Fresh,
    /// v12 及更早的 cache-inclusive 行：input 只包含 cache read。
    IncludesCacheRead,
    /// cache-inclusive（OpenAI Responses、Gemini 风格）：input 同时包含 cache read 与 cache write。
    IncludesCache,
}

impl InputBasis {
    /// 新写入的行：语义完全由 [`crate::app_config::AppType::input_semantics`] 决定。
    pub fn for_app_type(app_type: &str) -> Self {
        if InputSemantics::for_app_type_str(app_type).is_cache_inclusive() {
            InputBasis::IncludesCache
        } else {
            InputBasis::Fresh
        }
    }

    /// 已落库的行：再结合 `input_token_semantics` 区分 LEGACY / TOTAL / FRESH。
    /// cache-exclusive app 的行无论标记如何都按 fresh input 处理。
    pub fn for_stored_row(app_type: &str, input_token_semantics: i64) -> Self {
        if !InputSemantics::for_app_type_str(app_type).is_cache_inclusive()
            || input_token_semantics == INPUT_TOKEN_SEMANTICS_FRESH
        {
            InputBasis::Fresh
        } else if input_token_semantics == INPUT_TOKEN_SEMANTICS_TOTAL {
            InputBasis::IncludesCache
        } else {
            InputBasis::IncludesCacheRead
        }
    }

    fn billable_input(self, tokens: &TokenCounts) -> u64 {
        match self {
            InputBasis::Fresh => tokens.input,
            InputBasis::IncludesCacheRead => tokens.input.saturating_sub(tokens.cache_read),
            InputBasis::IncludesCache => tokens
                .input
                .saturating_sub(tokens.cache_read)
                .saturating_sub(tokens.cache_creation),
        }
    }
}

/// 计算请求成本——全项目唯一的成本函数。
///
/// - input_cost: 扣除 cache 部分后的 fresh input × 输入价格
/// - cache_read_cost / cache_creation_cost: 对应 token × 缓存读 / 写价格
/// - 各分项是基础成本（不含倍率）；total_cost = 分项之和 × 倍率
pub fn cost(
    tokens: &TokenCounts,
    basis: InputBasis,
    pricing: &ModelPricing,
    cost_multiplier: Decimal,
) -> CostBreakdown {
    let million = Decimal::from(1_000_000u64);

    let input_cost =
        Decimal::from(basis.billable_input(tokens)) * pricing.input_cost_per_million / million;
    let output_cost = Decimal::from(tokens.output) * pricing.output_cost_per_million / million;
    let cache_read_cost =
        Decimal::from(tokens.cache_read) * pricing.cache_read_cost_per_million / million;
    let cache_creation_cost =
        Decimal::from(tokens.cache_creation) * pricing.cache_creation_cost_per_million / million;

    let base_total = input_cost + output_cost + cache_read_cost + cache_creation_cost;
    let total_cost = base_total * cost_multiplier;

    CostBreakdown {
        input_cost,
        output_cost,
        cache_read_cost,
        cache_creation_cost,
        total_cost,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_LEGACY;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    const TOKENS: TokenCounts = TokenCounts {
        input: 1000,
        output: 500,
        cache_read: 200,
        cache_creation: 100,
    };

    #[test]
    fn pricing_engine_input_basis_deducts_expected_cache_buckets() {
        let pricing = ModelPricing::from_strings("3", "15", "0.3", "3.75").unwrap();
        let fresh = cost(&TOKENS, InputBasis::Fresh, &pricing, Decimal::ONE);
        let legacy = cost(
            &TOKENS,
            InputBasis::IncludesCacheRead,
            &pricing,
            Decimal::ONE,
        );
        let total = cost(&TOKENS, InputBasis::IncludesCache, &pricing, Decimal::ONE);
        assert_eq!(fresh.input_cost, d("0.003"));
        assert_eq!(legacy.input_cost, d("0.0024"));
        assert_eq!(total.input_cost, d("0.0021"));
        assert_eq!(fresh.total_cost, d("0.010935"));
        assert_eq!(total.total_cost, d("0.010035"));
    }

    /// 只有本文件可以读取每百万 token 单价做算术；其它路径必须调用 [`cost`]。
    #[test]
    fn pricing_engine_is_the_only_cost_function() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let this_file = root.join("src/pricing/engine.rs");
        let mut files = Vec::new();
        walk(&root.join("src"), &mut files);
        walk(&root.join("tests"), &mut files);

        let offenders: Vec<String> = files
            .iter()
            .filter(|path| **path != this_file)
            .filter(|path| {
                let text = std::fs::read_to_string(path).unwrap();
                [
                    ".input_cost_per_million",
                    ".output_cost_per_million",
                    ".cache_read_cost_per_million",
                    ".cache_creation_cost_per_million",
                ]
                .iter()
                .any(|field| text.contains(field))
            })
            .map(|path| path.display().to_string())
            .collect();
        assert!(
            offenders.is_empty(),
            "cost arithmetic outside pricing::engine::cost: {offenders:?}"
        );
    }

    #[test]
    fn pricing_engine_multiplier_only_scales_total() {
        let pricing = ModelPricing::from_strings("3", "0", "0", "0").unwrap();
        let tokens = TokenCounts {
            input: 1000,
            ..TokenCounts::default()
        };
        let c = cost(&tokens, InputBasis::Fresh, &pricing, d("1.5"));
        assert_eq!(c.input_cost, d("0.003"));
        assert_eq!(c.total_cost, d("0.0045"));
    }

    #[test]
    fn pricing_engine_saturates_when_cache_exceeds_input() {
        let pricing = ModelPricing::from_strings("10", "0", "0", "0").unwrap();
        let tokens = TokenCounts {
            input: 100,
            cache_read: 300,
            ..TokenCounts::default()
        };
        let c = cost(&tokens, InputBasis::IncludesCache, &pricing, Decimal::ONE);
        assert_eq!(c.input_cost, Decimal::ZERO);
    }

    #[test]
    fn pricing_engine_stored_row_basis_matches_ingest_basis_for_new_rows() {
        for app in crate::app_config::AppType::all() {
            let app = app.as_str();
            let stamped = crate::services::sql_helpers::input_token_semantics_for_app_type(app);
            assert_eq!(
                InputBasis::for_stored_row(app, stamped),
                InputBasis::for_app_type(app),
                "{app}"
            );
        }
        assert_eq!(
            InputBasis::for_stored_row("codex", INPUT_TOKEN_SEMANTICS_LEGACY),
            InputBasis::IncludesCacheRead
        );
        assert_eq!(
            InputBasis::for_stored_row("claude", INPUT_TOKEN_SEMANTICS_LEGACY),
            InputBasis::Fresh
        );
        assert_eq!(
            InputBasis::for_stored_row("not-an-app", INPUT_TOKEN_SEMANTICS_TOTAL),
            InputBasis::Fresh
        );
    }
}
