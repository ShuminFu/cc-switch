//! Cost Calculator - 计算 API 请求成本
//!
//! 只是 [`crate::pricing::engine::cost`] 的薄封装：把代理侧的 [`TokenUsage`]
//! 转成引擎的 token 计数，并按 app_type 选择 input 语义。

use super::parser::TokenUsage;
use crate::pricing::engine::{self, InputBasis, TokenCounts};
use rust_decimal::Decimal;

pub use crate::pricing::engine::{CostBreakdown, ModelPricing};

/// 成本计算器
pub struct CostCalculator;

impl CostCalculator {
    /// 计算请求成本（Claude/Anthropic 语义：input_tokens 已经不包含 cache 部分）
    ///
    /// 各项成本是基础成本（不含倍率），倍率只作用于最终总价。
    pub fn calculate(
        usage: &TokenUsage,
        pricing: &ModelPricing,
        cost_multiplier: Decimal,
    ) -> CostBreakdown {
        engine::cost(
            &token_counts(usage),
            InputBasis::Fresh,
            pricing,
            cost_multiplier,
        )
    }

    /// 按 app_type 选择输入 token 语义后计算成本。
    ///
    /// 语义由 [`crate::app_config::AppType::input_semantics`] 决定：
    /// Codex/OpenAI Responses、Gemini、Grok Build 的输入 token 字段包含 cache 部分；
    /// Claude/Anthropic 的 input_tokens 已经是 fresh input。
    pub fn calculate_for_app(
        app_type: &str,
        usage: &TokenUsage,
        pricing: &ModelPricing,
        cost_multiplier: Decimal,
    ) -> CostBreakdown {
        engine::cost(
            &token_counts(usage),
            InputBasis::for_app_type(app_type),
            pricing,
            cost_multiplier,
        )
    }

    pub fn try_calculate_for_app(
        app_type: &str,
        usage: &TokenUsage,
        pricing: Option<&ModelPricing>,
        cost_multiplier: Decimal,
    ) -> Option<CostBreakdown> {
        pricing.map(|p| Self::calculate_for_app(app_type, usage, p, cost_multiplier))
    }
}

fn token_counts(usage: &TokenUsage) -> TokenCounts {
    TokenCounts {
        input: u64::from(usage.input_tokens),
        output: u64::from(usage.output_tokens),
        cache_read: u64::from(usage.cache_read_tokens),
        cache_creation: u64::from(usage.cache_creation_tokens),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_cost_calculation() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_creation_tokens: 100,
            model: None,
            message_id: None,
        };

        let pricing = ModelPricing::from_strings("3.0", "15.0", "0.3", "3.75").unwrap();
        let multiplier = Decimal::from_str("1.0").unwrap();

        let cost = CostCalculator::calculate(&usage, &pricing, multiplier);

        // Claude/Anthropic 语义：input_tokens 已经不含 cache_read_tokens
        // input: 1000 * 3.0 / 1M = 0.003
        assert_eq!(cost.input_cost, Decimal::from_str("0.003").unwrap());
        // output: 500 * 15.0 / 1M = 0.0075
        assert_eq!(cost.output_cost, Decimal::from_str("0.0075").unwrap());
        // cache_read: 200 * 0.3 / 1M = 0.00006
        assert_eq!(cost.cache_read_cost, Decimal::from_str("0.00006").unwrap());
        // cache_creation: 100 * 3.75 / 1M = 0.000375
        assert_eq!(
            cost.cache_creation_cost,
            Decimal::from_str("0.000375").unwrap()
        );
        // total: 0.003 + 0.0075 + 0.00006 + 0.000375 = 0.010935
        assert_eq!(cost.total_cost, Decimal::from_str("0.010935").unwrap());
    }

    #[test]
    fn test_cost_calculation_for_cache_inclusive_app() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_creation_tokens: 100,
            model: None,
            message_id: None,
        };

        let pricing = ModelPricing::from_strings("3.0", "15.0", "0.3", "3.75").unwrap();
        let multiplier = Decimal::from_str("1.0").unwrap();

        let cost = CostCalculator::calculate_for_app("codex", &usage, &pricing, multiplier);

        // Codex/OpenAI 语义：input_tokens 包含 cache read/write，两桶都需扣除。
        assert_eq!(cost.input_cost, Decimal::from_str("0.0021").unwrap());
        assert_eq!(cost.output_cost, Decimal::from_str("0.0075").unwrap());
        assert_eq!(cost.cache_read_cost, Decimal::from_str("0.00006").unwrap());
        assert_eq!(
            cost.cache_creation_cost,
            Decimal::from_str("0.000375").unwrap()
        );
        assert_eq!(cost.total_cost, Decimal::from_str("0.010035").unwrap());
    }

    #[test]
    fn grokbuild_does_not_double_bill_cached_input() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 0,
            cache_read_tokens: 600,
            cache_creation_tokens: 0,
            model: None,
            message_id: None,
        };
        let pricing = ModelPricing::from_strings("10", "0", "1", "0").unwrap();

        let cost = CostCalculator::calculate_for_app("grokbuild", &usage, &pricing, Decimal::ONE);

        assert_eq!(cost.input_cost, Decimal::from_str("0.004").unwrap());
        assert_eq!(cost.cache_read_cost, Decimal::from_str("0.0006").unwrap());
        assert_eq!(cost.total_cost, Decimal::from_str("0.0046").unwrap());
    }

    #[test]
    fn test_cost_multiplier() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
            message_id: None,
        };

        let pricing = ModelPricing::from_strings("3.0", "15.0", "0", "0").unwrap();
        let multiplier = Decimal::from_str("1.5").unwrap();

        let cost = CostCalculator::calculate(&usage, &pricing, multiplier);

        // input_cost: 基础价格（不含倍率）= 1000 * 3.0 / 1M = 0.003
        assert_eq!(cost.input_cost, Decimal::from_str("0.003").unwrap());
        // total_cost: 基础价格 × 倍率 = 0.003 * 1.5 = 0.0045
        assert_eq!(cost.total_cost, Decimal::from_str("0.0045").unwrap());
    }

    #[test]
    fn test_unknown_model_handling() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
            message_id: None,
        };

        let multiplier = Decimal::from_str("1.0").unwrap();
        let cost = CostCalculator::try_calculate_for_app("claude", &usage, None, multiplier);

        assert!(cost.is_none());
    }

    #[test]
    fn test_decimal_precision() {
        let usage = TokenUsage {
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: 1,
            cache_creation_tokens: 1,
            model: None,
            message_id: None,
        };

        let pricing = ModelPricing::from_strings("0.075", "0.3", "0.01875", "0.075").unwrap();
        let multiplier = Decimal::from_str("1.0").unwrap();

        let cost = CostCalculator::calculate(&usage, &pricing, multiplier);

        // 验证高精度计算
        assert!(cost.total_cost > Decimal::ZERO);
        assert!(cost.total_cost.to_string().len() > 2); // 确保保留了小数位
    }
}
