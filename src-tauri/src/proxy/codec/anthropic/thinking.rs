//! Codex `reasoning.effort` → Anthropic thinking 配置的映射表

/// Maps Codex's reasoning.effort to the token budget for Anthropic thinking.
///
/// Returning `None` indicates an unrecognized effort value—in that case extended
/// thinking should not be enabled (to avoid accidentally swallowing
/// temperature/top_p), keeping normal sampling.
pub(crate) fn effort_to_thinking_budget(effort: &str) -> Option<u64> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "minimal" | "low" => Some(2048),
        "medium" => Some(8192),
        "high" => Some(16384),
        "xhigh" | "max" => Some(24576),
        _ => None,
    }
}

pub(crate) fn codex_effort_to_anthropic(effort: &str) -> Option<&'static str> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "minimal" | "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" | "max" => Some("max"),
        _ => None,
    }
}

pub(crate) fn reasoning_explicitly_disabled(effort: Option<&str>) -> bool {
    matches!(
        effort
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("none" | "off" | "disabled")
    )
}
