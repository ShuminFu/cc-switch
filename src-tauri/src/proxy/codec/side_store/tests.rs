use std::sync::Arc;

use serde_json::{json, Value};

use super::*;
use crate::proxy::providers::codex_chat_history::CodexChatHistoryStore;
use crate::proxy::providers::gemini_shadow::{
    GeminiAssistantTurn, GeminiShadowStore, GeminiToolCallMeta,
};
use crate::proxy::providers::reasoning_bridge::ReasoningBridge;

type GeminiSideStore =
    dyn SessionSideStore<Record = GeminiAssistantTurn, Restore = Vec<GeminiAssistantTurn>>;
type JsonSideStore = dyn SessionSideStore<Record = Value, Restore = Value>;

fn gemini_turn(text: &str, call_id: &str, signature: &str) -> GeminiAssistantTurn {
    GeminiAssistantTurn::new(
        json!({
            "role": "model",
            "parts": [
                { "text": text },
                {
                    "functionCall": { "id": call_id, "name": "read_file", "args": {} },
                    "thoughtSignature": signature
                }
            ]
        }),
        vec![GeminiToolCallMeta::new(
            Some(call_id),
            "read_file",
            json!({}),
            Some(signature),
        )],
    )
}

/// 编解码只拿到 trait 对象时也能完整记录与回放
#[test]
fn gemini_shadow_records_and_restores_through_trait_object() {
    let store = Arc::new(GeminiShadowStore::with_limits(8, 4));
    let side: Arc<GeminiSideStore> = store.clone();
    assert_eq!(side.kind(), SideStoreKind::GeminiShadow);

    let scope = SideStoreScope::new("prov", Some("sess"));
    assert_eq!(
        side.record(&scope, &mut gemini_turn("one", "call_1", "sig-1")),
        1
    );
    assert_eq!(
        side.record(&scope, &mut gemini_turn("two", "call_2", "sig-2")),
        1
    );

    let mut turns = Vec::new();
    assert_eq!(side.restore(&scope, &mut turns), 2);
    assert_eq!(turns[0], gemini_turn("one", "call_1", "sig-1"));
    assert_eq!(turns[1], gemini_turn("two", "call_2", "sig-2"));
    // 与具体类型的读接口看到的是同一份状态
    assert_eq!(store.get_session("prov", "sess").unwrap().turns, turns);
}

#[test]
fn gemini_shadow_is_scoped_by_provider_and_session() {
    let store = GeminiShadowStore::with_limits(8, 4);
    let scope = SideStoreScope::new("prov", Some("sess"));
    store.record(&scope, &mut gemini_turn("one", "call_1", "sig-1"));

    let mut turns = Vec::new();
    for other in [
        SideStoreScope::new("other", Some("sess")),
        SideStoreScope::new("prov", Some("other")),
        SideStoreScope::new("prov", None),
    ] {
        assert_eq!(store.restore(&other, &mut turns), 0);
        assert!(turns.is_empty());
    }

    // 没有会话 ID 时不记录
    let mut turn = gemini_turn("two", "call_2", "sig-2");
    assert_eq!(
        store.record(&SideStoreScope::new("prov", None), &mut turn),
        0
    );
    assert_eq!(store.session_count(), 1);
    assert_eq!(turn, gemini_turn("two", "call_2", "sig-2"));
}

/// 经 trait 写入的 shadow 能被旧转换器回放：thoughtSignature 补回 functionCall
#[test]
fn gemini_shadow_recorded_through_trait_is_replayed_by_legacy_encoder() {
    let store = GeminiShadowStore::with_limits(8, 4);
    store.record(
        &SideStoreScope::new("prov", Some("sess")),
        &mut gemini_turn("Let me read it.", "call_1", "sig-1"),
    );

    let request = json!({
        "model": "gemini-2.5-pro",
        "messages": [
            { "role": "user", "content": "Read README" },
            {
                "role": "assistant",
                "content": [
                    { "type": "text", "text": "Let me read it." },
                    { "type": "tool_use", "id": "call_1", "name": "read_file", "input": {} }
                ]
            },
            {
                "role": "user",
                "content": [{ "type": "tool_result", "tool_use_id": "call_1", "content": "ok" }]
            }
        ]
    });
    let encoded = crate::proxy::providers::transform_gemini::anthropic_to_gemini_with_shadow(
        request,
        Some(&store),
        Some("prov"),
        Some("sess"),
    )
    .unwrap();
    assert_eq!(
        encoded["contents"][1]["parts"][1]["thoughtSignature"],
        "sig-1"
    );
    assert_eq!(
        encoded["contents"][2]["parts"][0]["functionResponse"]["name"],
        "read_file"
    );
}

#[test]
fn codex_chat_history_restores_missing_function_call_through_trait_object() {
    let side: Arc<JsonSideStore> = Arc::new(CodexChatHistoryStore::default());
    assert_eq!(side.kind(), SideStoreKind::CodexChatHistory);
    let scope = SideStoreScope::new("prov", None);

    let mut response = json!({
        "id": "resp_1",
        "output": [{
            "type": "function_call",
            "call_id": "call_1",
            "name": "read_file",
            "arguments": "{}",
            "reasoning_content": "Need the file."
        }]
    });
    let recorded = response.clone();
    assert_eq!(side.record(&scope, &mut response), 1);
    assert_eq!(response, recorded, "record must not modify the response");

    let mut request = json!({
        "previous_response_id": "resp_1",
        "input": [{ "type": "function_call_output", "call_id": "call_1", "output": "ok" }]
    });
    assert_eq!(side.restore(&scope, &mut request), 1);
    assert_eq!(request["input"][0]["type"], "function_call");
    assert_eq!(request["input"][0]["reasoning_content"], "Need the file.");
    assert_eq!(request["input"][1]["type"], "function_call_output");

    // 没有可补全的内容时原样保留
    let mut unrelated = json!({ "input": [{ "role": "user", "content": "hi" }] });
    let before = unrelated.clone();
    assert_eq!(side.restore(&scope, &mut unrelated), 0);
    assert_eq!(unrelated, before);
}

#[test]
fn reasoning_bridge_round_trips_item_through_client_carried_block() {
    let side: &JsonSideStore = &ReasoningBridge;
    assert_eq!(side.kind(), SideStoreKind::ReasoningBridge);
    let scope = SideStoreScope::new("prov", Some("sess"));

    let item = json!({
        "id": "rs_1",
        "type": "reasoning",
        "summary": [{ "type": "summary_text", "text": "Need a tool." }],
        "encrypted_content": "opaque"
    });
    let mut payload = item.clone();
    assert_eq!(side.record(&scope, &mut payload), 1);
    assert_eq!(payload["type"], "thinking");
    assert_eq!(payload["thinking"], "Need a tool.");

    // 客户端带回的块在另一个作用域（无会话）下同样能还原：状态在块本身
    assert_eq!(
        side.restore(&SideStoreScope::new("other", None), &mut payload),
        1
    );
    assert_eq!(payload, item);
}

#[test]
fn reasoning_bridge_leaves_unconvertible_payloads_untouched() {
    let scope = SideStoreScope::new("prov", None);

    let mut plain_thinking = json!({ "type": "thinking", "thinking": "local", "signature": "sig" });
    let before = plain_thinking.clone();
    assert_eq!(ReasoningBridge.restore(&scope, &mut plain_thinking), 0);
    assert_eq!(plain_thinking, before);

    let mut empty_item = json!({ "type": "reasoning", "summary": [] });
    let before = empty_item.clone();
    assert_eq!(ReasoningBridge.record(&scope, &mut empty_item), 0);
    assert_eq!(empty_item, before);
}

#[test]
fn kind_names_match_route_plan_side_store_flags() {
    assert_eq!(SideStoreKind::GeminiShadow.as_str(), "gemini_shadow");
    assert_eq!(
        SideStoreKind::CodexChatHistory.as_str(),
        "codex_chat_history"
    );
    assert_eq!(SideStoreKind::ReasoningBridge.as_str(), "reasoning_bridge");
}
