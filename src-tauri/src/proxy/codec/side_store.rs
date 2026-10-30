//! 会话旁路存储：IR 之外、跨请求保留的协议私有状态
//!
//! IR 只表达各线协议共有的语义。有些状态只有某一种协议需要，客户端又不会原样带回，
//! 只能由代理在回复侧记住、在下一次请求时补全：
//! - [`GeminiShadowStore`](crate::proxy::providers::gemini_shadow::GeminiShadowStore)
//!   — Gemini 原始助手 parts 与 functionCall 的 `thoughtSignature`，Anthropic
//!   客户端回放历史时按 (provider, 会话) 补回
//! - [`CodexChatHistoryStore`](crate::proxy::providers::codex_chat_history::CodexChatHistoryStore)
//!   — Codex Responses → Chat 时，按 `previous_response_id` / `call_id` 补回
//!   请求里缺失的历史 function call（含 `reasoning_content`）
//! - [`ReasoningBridge`](crate::proxy::providers::reasoning_bridge::ReasoningBridge)
//!   — Responses `reasoning` 条目装进 Anthropic thinking 签名，由客户端携带往返；
//!   不占用代理内存，但与前两者走同一个接口
//!
//! 三者都实现 [`SessionSideStore`]：回复侧 [`SessionSideStore::record`]，请求侧
//! [`SessionSideStore::restore`]，作用域统一为 [`SideStoreScope`]。需要旁路状态的
//! 编解码在构造时接收对应存储（`Arc<S>` 或 `&dyn SessionSideStore<..>`）与作用域，
//! 不在 IR 中夹带协议私有字段；旧转换器经同一接口读写，保证迁移前后行为一致。

/// 旁路存储类型；名字与 [`crate::proxy::routing::SideStores`] 的字段一致
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SideStoreKind {
    /// Claude → Gemini Native 的 shadow 回放
    GeminiShadow,
    /// Codex Responses → Chat 的历史 function call
    CodexChatHistory,
    /// Responses reasoning 条目 ⇄ Anthropic thinking 签名
    ReasoningBridge,
}

impl SideStoreKind {
    /// 日志 / trace 中使用的名字
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GeminiShadow => "gemini_shadow",
            Self::CodexChatHistory => "codex_chat_history",
            Self::ReasoningBridge => "reasoning_bridge",
        }
    }
}

/// 旁路状态的作用域：一次请求所属的 provider 与客户端会话
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideStoreScope<'a> {
    pub provider_id: &'a str,
    /// 客户端会话 ID；为 `None` 时按会话隔离的存储既不记录也不回填
    pub session_id: Option<&'a str>,
}

impl<'a> SideStoreScope<'a> {
    pub fn new(provider_id: &'a str, session_id: Option<&'a str>) -> Self {
        Self {
            provider_id,
            session_id,
        }
    }
}

/// 跨请求旁路存储
///
/// `Record` / `Restore` 是各存储读写的协议私有载荷（见各实现的文档）。两个方法都按
/// 可变引用接收载荷：有状态的存储在 `record` 中只读、在 `restore` 中原地补全；
/// 无状态的 [`ReasoningBridge`](crate::proxy::providers::reasoning_bridge::ReasoningBridge)
/// 把状态编码进载荷本身。返回值是记录或补全的条目数，0 表示没有改动（载荷保持原样）。
///
/// 实现必须是同步且短时持锁的：编解码在请求路径上同步调用它们。
pub trait SessionSideStore: Send + Sync {
    /// 回复侧交给存储的载荷
    type Record: ?Sized;
    /// 请求侧由存储补全的载荷
    type Restore: ?Sized;

    fn kind(&self) -> SideStoreKind;

    /// 回复侧：记住 `record` 中下一次请求需要、客户端又不会带回的状态
    fn record(&self, scope: &SideStoreScope<'_>, record: &mut Self::Record) -> usize;

    /// 请求侧：把之前记住的状态补回 `restore`
    fn restore(&self, scope: &SideStoreScope<'_>, restore: &mut Self::Restore) -> usize;
}

#[cfg(test)]
mod tests;
