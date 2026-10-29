//! StopReason - 各协议停止原因的规范表
//!
//! 流式与非流式路径曾各自维护一份 finish_reason → stop_reason 映射，彼此逐渐漂移
//! （例如 Gemini 未知 finishReason + 工具调用：流式回落为 tool_use，非流式为 end_turn）。
//! 现每种上游格式只保留一张表（`from_<format>`），两条路径共用。
//!
//! tool_use 覆盖规则（CCS-065）：上游已产出工具调用、而表映射为 end_turn 时，一律
//! 改为 tool_use，否则 Claude Code 会结束回合而不执行工具。max_tokens / refusal 保持不变。

/// 规范化的停止原因（以 Anthropic Messages 的取值为准）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    Refusal,
}

impl StopReason {
    /// Anthropic `stop_reason` 字符串
    pub fn as_anthropic_str(self) -> &'static str {
        match self {
            Self::EndTurn => "end_turn",
            Self::MaxTokens => "max_tokens",
            Self::ToolUse => "tool_use",
            Self::Refusal => "refusal",
        }
    }

    /// tool_use 覆盖：已产出工具调用时 end_turn → tool_use
    fn with_tool_use(self, has_tool_use: bool) -> Self {
        match self {
            Self::EndTurn if has_tool_use => Self::ToolUse,
            other => other,
        }
    }

    /// OpenAI Chat Completions `finish_reason`
    pub fn from_chat_finish_reason(finish_reason: &str, has_tool_use: bool) -> Self {
        let base = match finish_reason {
            "tool_calls" | "function_call" => Self::ToolUse,
            "stop" => Self::EndTurn,
            "length" => Self::MaxTokens,
            "content_filter" => Self::EndTurn,
            other => {
                log::warn!("[Claude/OpenAI] Unknown finish_reason `{other}`, using end_turn");
                Self::EndTurn
            }
        };
        base.with_tool_use(has_tool_use)
    }

    /// Gemini `finishReason`（缺省按 STOP 处理）
    pub fn from_gemini_finish_reason(finish_reason: Option<&str>, has_tool_use: bool) -> Self {
        let base = match finish_reason {
            Some("MAX_TOKENS") => Self::MaxTokens,
            Some("STOP") | Some("FINISH_REASON_UNSPECIFIED") | None => Self::EndTurn,
            Some("SAFETY")
            | Some("RECITATION")
            | Some("SPII")
            | Some("BLOCKLIST")
            | Some("PROHIBITED_CONTENT") => Self::Refusal,
            Some(other) => {
                log::warn!("[Claude/Gemini] Unknown Gemini finishReason `{other}`, using end_turn");
                Self::EndTurn
            }
        };
        base.with_tool_use(has_tool_use)
    }

    /// OpenAI Responses `status` + `incomplete_details.reason`
    ///
    /// 仅 `completed` 应用 tool_use 覆盖：`incomplete` 时工具调用可能不完整，
    /// 不能让客户端去执行。
    pub fn from_responses_status(
        status: &str,
        has_tool_use: bool,
        incomplete_reason: Option<&str>,
    ) -> Self {
        match status {
            "completed" => Self::EndTurn.with_tool_use(has_tool_use),
            "incomplete"
                if matches!(
                    incomplete_reason,
                    None | Some("max_output_tokens") | Some("max_tokens")
                ) =>
            {
                Self::MaxTokens
            }
            _ => Self::EndTurn,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_table() {
        let cases = [
            ("stop", false, "end_turn"),
            ("stop", true, "tool_use"),
            ("tool_calls", false, "tool_use"),
            ("function_call", false, "tool_use"),
            ("length", false, "max_tokens"),
            ("length", true, "max_tokens"),
            ("content_filter", false, "end_turn"),
            ("content_filter", true, "tool_use"),
            ("something_new", false, "end_turn"),
            ("something_new", true, "tool_use"),
        ];
        for (reason, has_tool_use, expected) in cases {
            assert_eq!(
                StopReason::from_chat_finish_reason(reason, has_tool_use).as_anthropic_str(),
                expected,
                "{reason} has_tool_use={has_tool_use}"
            );
        }
    }

    #[test]
    fn gemini_table() {
        let cases = [
            (None, false, "end_turn"),
            (None, true, "tool_use"),
            (Some("STOP"), false, "end_turn"),
            (Some("STOP"), true, "tool_use"),
            (Some("FINISH_REASON_UNSPECIFIED"), true, "tool_use"),
            (Some("MAX_TOKENS"), true, "max_tokens"),
            (Some("SAFETY"), true, "refusal"),
            (Some("RECITATION"), false, "refusal"),
            (Some("SPII"), false, "refusal"),
            (Some("BLOCKLIST"), false, "refusal"),
            (Some("PROHIBITED_CONTENT"), false, "refusal"),
            (Some("MALFORMED_FUNCTION_CALL"), false, "end_turn"),
            (Some("MALFORMED_FUNCTION_CALL"), true, "tool_use"),
        ];
        for (reason, has_tool_use, expected) in cases {
            assert_eq!(
                StopReason::from_gemini_finish_reason(reason, has_tool_use).as_anthropic_str(),
                expected,
                "{reason:?} has_tool_use={has_tool_use}"
            );
        }
    }

    #[test]
    fn responses_table() {
        let cases = [
            ("completed", false, None, "end_turn"),
            ("completed", true, None, "tool_use"),
            ("incomplete", false, None, "max_tokens"),
            ("incomplete", true, Some("max_output_tokens"), "max_tokens"),
            ("incomplete", false, Some("max_tokens"), "max_tokens"),
            ("incomplete", true, Some("content_filter"), "end_turn"),
            ("in_progress", true, None, "end_turn"),
        ];
        for (status, has_tool_use, incomplete_reason, expected) in cases {
            assert_eq!(
                StopReason::from_responses_status(status, has_tool_use, incomplete_reason)
                    .as_anthropic_str(),
                expected,
                "{status} has_tool_use={has_tool_use} reason={incomplete_reason:?}"
            );
        }
    }
}
