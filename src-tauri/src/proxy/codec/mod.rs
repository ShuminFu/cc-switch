//! 每种线协议一套编解码，经 [`crate::proxy::protocol::ir`] 拼接为任意桥接
//!
//! 一种格式的编解码分三部分：
//! - [`RequestCodec`] — 请求体 ⇄ [`ChatRequest`]
//! - [`ResponseCodec`] — 非流式响应体 ⇄ [`ChatResponse`]
//! - [`StreamCodec`] — SSE 事件 ⇄ [`StreamEvent`]（有状态，每个流新建一对）
//!
//! 非流式即「流式 + 聚合」：[`Codec::aggregate_stream`] 把上游 SSE 解码后经
//! [`StreamAggregator`] 折叠为完整回复（流式上游 → 非流式客户端），
//! [`Codec::encode_response_as_stream`] 把完整回复展开为事件后编码为 SSE
//! （非流式上游 → 流式客户端），两者都不需要格式各自再写一遍。
//!
//! 迁移期间由 [`CodecEngine`]（设置项 `codecEngine`）决定桥接走旧转换器还是编解码；
//! 各桥接按 anthropic←openai_chat、gemini、responses 的顺序逐个迁移，入口见 [`bridge`]。
//! 已迁移：anthropic←openai_chat（[`anthropic`] + [`openai_chat`]，CCS-176）、
//! anthropic←gemini（[`anthropic`] + [`gemini`]，CCS-177）、
//! anthropic←responses（[`anthropic`] + [`responses`]，CCS-178）。

// 桥接逐个迁移到编解码；尚未迁移的桥接对应的部分 IR 与编解码项暂无调用方。
#![allow(dead_code)]

use bytes::Bytes;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::proxy::protocol::ir::{
    AggregateError, ChatRequest, ChatResponse, StreamAggregator, StreamEvent,
};
use crate::proxy::protocol::WireFormat;

pub mod anthropic;
pub mod bridge;
pub mod gemini;
pub mod openai_chat;
pub mod responses;
pub mod side_store;
pub mod transcode;

/// 协议转换引擎
///
/// `Legacy` 保留一个版本作为回退：在编解码出现回归时让所有桥接退回原有的
/// transform* / streaming* 转换器。尚未迁移的桥接无论取值如何都走旧转换器。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodecEngine {
    /// 原有的逐桥接转换器
    #[default]
    Legacy,
    /// 基于 IR 的编解码（仅对已迁移的桥接生效）
    Ir,
}

impl CodecEngine {
    /// 设置中使用的字符串形式
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Ir => "ir",
        }
    }

    /// 宽松解析：未知值回退为 [`CodecEngine::Legacy`]，避免一个未知取值
    /// 让整个设置文件解析失败
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "ir" => Self::Ir,
            "legacy" => Self::Legacy,
            other => {
                log::warn!("[Codec] Unknown codecEngine `{other}`, using legacy");
                Self::Legacy
            }
        }
    }

    pub fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy)
    }

    /// 某个桥接是否应走 IR 编解码：桥接已迁移且未强制使用 legacy
    pub fn uses_ir(self, bridge_migrated: bool) -> bool {
        bridge_migrated && self == Self::Ir
    }
}

impl Serialize for CodecEngine {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CodecEngine {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // 非字符串（null、数字等）同样回退为 legacy，而不是让整个设置解析失败
        match Value::deserialize(deserializer)? {
            Value::String(value) => Ok(Self::from_setting(&value)),
            other => {
                log::warn!("[Codec] Non-string codecEngine `{other}`, using legacy");
                Ok(Self::Legacy)
            }
        }
    }
}

/// JSON 数字：整数值的浮点按整数输出（客户端发 `1` 时上游收到 `1` 而不是 `1.0`）
pub(crate) fn json_number(value: f64) -> Value {
    const MAX_EXACT: f64 = 9_007_199_254_740_992.0; // 2^53
    if value.fract() == 0.0 && value.abs() < MAX_EXACT {
        serde_json::json!(value as i64)
    } else {
        serde_json::json!(value)
    }
}

/// 编解码错误
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    /// 输入不符合该格式（缺字段、类型错误、JSON 无效）
    #[error("invalid {format:?} payload: {message}")]
    Invalid { format: WireFormat, message: String },
    /// 目标格式无法表达 IR 中的某项内容
    #[error("{format:?} cannot represent {what}")]
    Unsupported { format: WireFormat, what: String },
    /// 解码出的事件序列违反 [`StreamEvent`] 约定
    #[error(transparent)]
    Aggregate(#[from] AggregateError),
}

/// 请求编解码
pub trait RequestCodec: Send + Sync {
    /// 本编解码对应的线协议
    fn format(&self) -> WireFormat;
    /// 请求体 → IR
    fn decode_request(&self, body: &Value) -> Result<ChatRequest, CodecError>;
    /// IR → 请求体
    fn encode_request(&self, request: &ChatRequest) -> Result<Value, CodecError>;
}

/// 非流式响应编解码
pub trait ResponseCodec: Send + Sync {
    /// 响应体 → IR
    fn decode_response(&self, body: &Value) -> Result<ChatResponse, CodecError>;
    /// IR → 响应体
    fn encode_response(&self, response: &ChatResponse) -> Result<Value, CodecError>;
}

/// 单个流的 SSE 解码状态机：上游 SSE 事件 → IR 事件
pub trait EventDecoder: Send {
    /// 解码一个 SSE 事件（`event` 为事件名，缺省时为空串；`data` 为合并后的数据行）
    fn decode(&mut self, event: &str, data: &str) -> Result<Vec<StreamEvent>, CodecError>;
    /// 上游流结束时补发的事件（例如上游没有显式结束事件的格式）
    fn finish(&mut self) -> Result<Vec<StreamEvent>, CodecError> {
        Ok(Vec::new())
    }
}

/// 单个流的 SSE 编码状态机：IR 事件 → 客户端 SSE 帧
pub trait EventEncoder: Send {
    /// 编码一个事件为零到多个完整 SSE 帧
    fn encode(&mut self, event: &StreamEvent) -> Vec<Bytes>;
    /// 事件序列结束时补发的帧（例如 `data: [DONE]`）
    fn finish(&mut self) -> Vec<Bytes> {
        Vec::new()
    }
}

/// 流式编解码：每个流新建一对状态机
///
/// 一种格式只在某一侧出现时（例如 OpenAI Chat 只作为上游），未实现的方向
/// 返回 [`CodecError::Unsupported`]。
pub trait StreamCodec: Send + Sync {
    fn stream_decoder(&self) -> Result<Box<dyn EventDecoder>, CodecError>;
    fn stream_encoder(&self) -> Result<Box<dyn EventEncoder>, CodecError>;
}

/// 一种线协议的完整编解码；非流式与流式之间的转换由提供的方法完成
pub trait Codec: RequestCodec + ResponseCodec + StreamCodec {
    /// 流式上游 → 完整回复：逐个解码 SSE 事件并聚合
    fn aggregate_stream<'a>(
        &self,
        events: &mut dyn Iterator<Item = (&'a str, &'a str)>,
    ) -> Result<ChatResponse, CodecError> {
        let mut decoder = self.stream_decoder()?;
        let mut aggregator = StreamAggregator::new();
        for (event, data) in events {
            for ir_event in decoder.decode(event, data)? {
                aggregator.push(ir_event)?;
            }
        }
        for ir_event in decoder.finish()? {
            aggregator.push(ir_event)?;
        }
        Ok(aggregator.finish())
    }

    /// 完整回复 → 客户端 SSE 帧：展开为 IR 事件后逐个编码
    fn encode_response_as_stream(&self, response: &ChatResponse) -> Result<Vec<Bytes>, CodecError> {
        let mut encoder = self.stream_encoder()?;
        let mut frames: Vec<Bytes> = response
            .to_events()
            .iter()
            .flat_map(|event| encoder.encode(event))
            .collect();
        frames.extend(encoder.finish());
        Ok(frames)
    }
}

impl<T: RequestCodec + ResponseCodec + StreamCodec> Codec for T {}

#[cfg(test)]
mod tests;
