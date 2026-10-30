//! Body stages: model mapping and provider-specific request optimizers
//! ([`RequestForwarder::map_request_body`]), protocol transforms
//! ([`RequestForwarder::transform_request_body`]) and the final outbound body
//! policy ([`prepare_upstream_request_body`]).

use super::endpoint::Route;
use crate::app_config::AppType;
use crate::provider::{LocalProxyRequestOverrides, Provider};
use crate::proxy::copilot_optimizer::{self, CopilotClassification};
use crate::proxy::forwarder::RequestForwarder;
use crate::proxy::json_canonical::{canonicalize_value, short_value_hash};
use crate::proxy::providers::{self, ProviderAdapter};
use crate::proxy::types::OptimizerConfig;
use crate::proxy::ProxyError;
use serde_json::Value;

pub(crate) const CLAUDE_CODE_SYSTEM_IDENTITY: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

/// Anthropic requires `max_tokens`; used when a Codex request omits
/// `max_output_tokens`. 8192 is accepted by every current Claude model and
/// virtually all gateways (a too-high default 400s and is non-retryable).
const DEFAULT_CODEX_ANTHROPIC_MAX_TOKENS: u64 = 8192;

/// Copilot optimizer results that also shape the request headers.
pub(crate) struct CopilotHints {
    pub classification: CopilotClassification,
    pub deterministic_request_id: Option<String>,
    pub interaction_id: Option<String>,
}

/// Output of [`RequestForwarder::transform_request_body`].
pub(crate) struct TransformedBody {
    /// Final outbound body (private keys stripped, canonical key order).
    pub body: Value,
    /// Codex→Anthropic: the model carried the `[1m]` marker (adds the 1M beta).
    pub codex_anthropic_one_m: bool,
}

impl RequestForwarder {
    /// Model mapping and request optimizers that run before any protocol transform.
    pub(crate) async fn map_request_body(
        &self,
        app_type: &AppType,
        provider: &Provider,
        route: &Route,
        body: &Value,
        headers: &http::HeaderMap,
    ) -> Result<(Value, Option<CopilotHints>), ProxyError> {
        // Claude Desktop must map its visible claude-* route to the real upstream
        // model, and unknown routes are errors (no default-model fallback).
        let mut mapped = if matches!(app_type, AppType::ClaudeDesktop) {
            crate::claude_desktop_config::map_proxy_request_model(body.clone(), provider)
                .map_err(|e| ProxyError::InvalidRequest(e.to_string()))?
        } else {
            crate::proxy::model_mapper::apply_model_mapping(body.clone(), provider).0
        };

        // Grok Build exposes a stable client-side model profile; route to the
        // provider's real model before the optional Responses bridge.
        if matches!(app_type, AppType::GrokBuild) {
            providers::apply_codex_upstream_model(provider, &mut mapped);
        }

        if route.is_copilot {
            mapped = providers::copilot_model_map::apply_copilot_model_normalization(mapped);
            self.apply_copilot_live_model_resolution(provider, &mut mapped)
                .await;
        } else if !route.codex_responses_to_anthropic {
            // On the Codex→Anthropic path the [1m] marker must survive to catalog
            // matching and the transform's own strip + beta detection.
            mapped = crate::proxy::model_mapper::strip_one_m_suffix_for_upstream_from_body(mapped);
        }

        let hints = if route.is_copilot && self.copilot_optimizer_config.enabled {
            Some(self.optimize_copilot_body(&mut mapped, body, headers))
        } else {
            None
        };
        Ok((mapped, hints))
    }

    /// Copilot optimizer (order matches copilot-api): classify on the original
    /// body, then drop orphan tool results, merge tool results, strip thinking and
    /// downgrade warmups.
    fn optimize_copilot_body(
        &self,
        mapped: &mut Value,
        client_body: &Value,
        headers: &http::HeaderMap,
    ) -> CopilotHints {
        let config = &self.copilot_optimizer_config;
        let classification = copilot_optimizer::classify_request(
            mapped,
            headers.contains_key("anthropic-beta"),
            config.compact_detection,
            config.subagent_detection,
        );
        log::debug!(
            "[Copilot] classification: initiator={}, is_warmup={}, is_compact={}, is_subagent={}",
            classification.initiator,
            classification.is_warmup,
            classification.is_compact,
            classification.is_subagent
        );

        let mut body = std::mem::take(mapped);
        body = copilot_optimizer::sanitize_orphan_tool_results(body);
        if config.tool_result_merging {
            body = copilot_optimizer::merge_tool_results(body);
        }
        if config.strip_thinking {
            body = copilot_optimizer::strip_thinking_blocks(body);
        }
        if config.warmup_downgrade && classification.is_warmup {
            log::info!(
                "[Copilot] warmup request downgraded to model {}",
                config.warmup_model
            );
            body["model"] = serde_json::json!(&config.warmup_model);
        }

        let session_id = copilot_session_id(client_body, headers);
        let deterministic_request_id = config
            .deterministic_request_id
            .then(|| copilot_optimizer::deterministic_request_id(&body, &session_id));
        let interaction_id = copilot_optimizer::deterministic_interaction_id(&session_id);
        *mapped = body;
        CopilotHints {
            classification,
            deterministic_request_id,
            interaction_id,
        }
    }

    /// Convert the mapped body to the upstream protocol and apply the outbound
    /// body policy (local overrides, private-key stripping, canonical order).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn transform_request_body(
        &self,
        app_type: &AppType,
        provider: &Provider,
        route: &Route,
        adapter: &dyn ProviderAdapter,
        mapped: Value,
        claude_api_format: Option<&str>,
        needs_transform: bool,
    ) -> Result<TransformedBody, ProxyError> {
        let mut codex_anthropic_one_m = false;
        // The internal session key stays the prompt-cache identity: changing it
        // would cold-start every existing conversation's upstream cache.
        let client_session = self
            .session_client_provided
            .then_some(self.session_id.as_str());

        let mut request_body = if route.codex_responses_to_chat {
            self.codex_responses_to_chat_body(provider, mapped, client_session)
                .await?
        } else if route.codex_responses_to_anthropic {
            let (body, one_m) = self.codex_responses_to_anthropic_body(provider, route, mapped)?;
            codex_anthropic_one_m = one_m;
            body
        } else if needs_transform && adapter.name() == "Claude" {
            let api_format =
                claude_api_format.unwrap_or_else(|| providers::get_claude_api_format(provider));
            providers::transform_claude_request_for_api_format(
                mapped,
                provider,
                api_format,
                client_session,
                Some(self.gemini_shadow.as_ref()),
            )?
        } else if needs_transform {
            adapter.transform_request(mapped, provider)?
        } else {
            mapped
        };

        if matches!(app_type, AppType::Codex | AppType::GrokBuild) {
            self.apply_media_prevention(&mut request_body, provider);
        }

        // Local body overrides are merged before the single strip + canonicalize
        // pass (stripping only removes `_` keys and commutes with the merge).
        if !route.is_copilot {
            if let Some(overrides) = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.local_proxy_request_overrides.as_ref())
            {
                apply_local_proxy_body_overrides(&mut request_body, overrides);
            }
        }
        Ok(TransformedBody {
            body: prepare_upstream_request_body(request_body),
            codex_anthropic_one_m,
        })
    }

    async fn codex_responses_to_chat_body(
        &self,
        provider: &Provider,
        mut mapped: Value,
        client_session: Option<&str>,
    ) -> Result<Value, ProxyError> {
        let explicit_prompt_cache_key = mapped
            .get("prompt_cache_key")
            .and_then(|value| value.as_str())
            .map(ToString::to_string);
        let restored = self.codex_chat_history.enrich_request(&mut mapped).await;
        if restored > 0 {
            log::debug!(
                "[Codex] Restored or enriched {restored} cached function call item(s) for Chat upstream"
            );
        }
        providers::apply_codex_chat_upstream_model(provider, &mut mapped);
        let reasoning_config = providers::resolve_codex_chat_reasoning_config(provider, &mapped);
        let mut chat_body =
            providers::transform_codex_chat::responses_to_chat_completions_with_reasoning(
                mapped,
                reasoning_config.as_ref(),
            )?;
        providers::inject_codex_chat_prompt_cache_key(
            provider,
            &mut chat_body,
            explicit_prompt_cache_key.as_deref(),
            client_session,
        );
        Ok(chat_body)
    }

    fn codex_responses_to_anthropic_body(
        &self,
        provider: &Provider,
        route: &Route,
        mut mapped: Value,
    ) -> Result<(Value, bool), ProxyError> {
        providers::apply_codex_upstream_model(provider, &mut mapped);
        // Per-provider output ceiling: Codex does not forward its
        // `model_max_output_tokens`, so the provider's value wins over the request
        // and the default; injecting it lets the thinking-budget clamp use it too.
        if let Some(max_out) = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.max_output_tokens)
            .filter(|v| *v > 0)
        {
            mapped["max_output_tokens"] = Value::from(max_out);
        }
        let mut anthropic_body =
            providers::transform_codex_anthropic::responses_request_to_anthropic(
                mapped,
                DEFAULT_CODEX_ANTHROPIC_MAX_TOKENS,
            )?;
        // The gateway does not know the [1m] marker: strip it from the final model
        // (apply_codex_upstream_model may have written it back) and add the beta.
        let mut one_m = false;
        if let Some(model) = anthropic_body.get("model").and_then(|v| v.as_str()) {
            let stripped = crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(model);
            if stripped != model {
                one_m = true;
                anthropic_body["model"] = Value::String(stripped.to_string());
            }
        }
        if route.codex_impersonate_claude_code {
            prepend_claude_code_system_prompt(&mut anthropic_body);
        }
        // Prompt caching is part of the bridge: without it system/tools/history
        // are re-sent at full price every round.
        crate::proxy::cache_injector::inject(
            &mut anthropic_body,
            &codex_anthropic_cache_config(&self.optimizer_config),
        );
        Ok((anthropic_body, one_m))
    }
}

/// Session key used by the Copilot optimizer (same priority as
/// `session::extract_from_metadata`): `metadata.user_id` `_session_` suffix,
/// `metadata.session_id`, the raw `metadata.user_id`, then `x-session-id`.
fn copilot_session_id(body: &Value, headers: &http::HeaderMap) -> String {
    let metadata = body.get("metadata");
    let user_id = metadata
        .and_then(|m| m.get("user_id"))
        .and_then(|v| v.as_str());
    user_id
        .and_then(crate::proxy::session::parse_session_from_user_id)
        .or_else(|| {
            metadata
                .and_then(|m| m.get("session_id"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .or_else(|| user_id.filter(|s| !s.is_empty()).map(str::to_string))
        .or_else(|| {
            headers
                .get("x-session-id")
                .and_then(|v| v.to_str().ok())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// Insert the Claude Code identity as the first `system` block (Anthropic
/// subscription/OAuth plans require exactly this first line). Idempotent.
pub(crate) fn prepend_claude_code_system_prompt(body: &mut Value) {
    let identity = serde_json::json!({ "type": "text", "text": CLAUDE_CODE_SYSTEM_IDENTITY });
    let mut blocks: Vec<Value> = vec![identity];
    match body.get("system") {
        Some(Value::String(existing)) if !existing.is_empty() => {
            blocks.push(serde_json::json!({ "type": "text", "text": existing }));
        }
        Some(Value::Array(existing)) => {
            if existing
                .first()
                .and_then(|b| b.get("text"))
                .and_then(|t| t.as_str())
                == Some(CLAUDE_CODE_SYSTEM_IDENTITY)
            {
                return;
            }
            blocks.extend(existing.iter().cloned());
        }
        _ => {}
    }
    body["system"] = Value::Array(blocks);
}

/// Prompt caching is part of the Codex→Anthropic bridge rather than an optional
/// Bedrock optimizer: on by default, honoring the dedicated cache-injection switch.
pub(crate) fn codex_anthropic_cache_config(config: &OptimizerConfig) -> OptimizerConfig {
    OptimizerConfig {
        enabled: true,
        thinking_optimizer: false,
        cache_injection: config.cache_injection,
    }
}

/// Final outbound body policy: strip proxy-private parameters and canonicalize
/// the key order (stable bytes keep upstream prompt caches warm).
///
/// Only the request's own top-level keys (and `metadata`'s) starting with `_` are
/// stripped (PROXY-37): message content, tool inputs/results, function-call
/// arguments and tool schemas are model- or user-owned payloads whose `_id` /
/// `_meta` keys must survive the round trip.
pub(crate) fn prepare_upstream_request_body(request_body: Value) -> Value {
    canonicalize_value(strip_private_top_level_params(request_body))
}

fn strip_private_top_level_params(mut body: Value) -> Value {
    if let Some(map) = body.as_object_mut() {
        let before = map.len();
        map.retain(|key, _| !key.starts_with('_'));
        if map.len() != before {
            log::debug!(
                "[BodyFilter] removed {} private top-level field(s)",
                before - map.len()
            );
        }
        // `metadata` belongs to the client↔proxy contract: strip private keys
        // recursively there.
        if let Some(metadata) = map.get_mut("metadata") {
            if metadata.is_object() {
                *metadata = crate::proxy::body_filter::filter_private_params_with_whitelist(
                    std::mem::take(metadata),
                    &[],
                );
            }
        }
    }
    body
}

pub(crate) fn apply_local_proxy_body_overrides(
    body: &mut Value,
    overrides: &LocalProxyRequestOverrides,
) -> bool {
    let Some(override_body) = overrides.body.as_ref() else {
        return false;
    };

    if !override_body.is_object() {
        log::warn!("[LocalProxyOverrides] Ignoring body override because it is not an object");
        return false;
    }

    merge_json_override(body, override_body, true)
}

fn merge_json_override(target: &mut Value, patch: &Value, is_top_level: bool) -> bool {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            let mut changed = false;
            for (key, patch_value) in patch_map {
                if is_top_level && key == "stream" {
                    log::warn!(
                        "[LocalProxyOverrides] Ignoring body override for protected field: stream"
                    );
                    continue;
                }
                match target_map.get_mut(key) {
                    Some(target_value) => {
                        changed |= merge_json_override(target_value, patch_value, false);
                    }
                    None => {
                        target_map.insert(key.clone(), patch_value.clone());
                        changed = true;
                    }
                }
            }
            changed
        }
        (target_value, patch_value) => {
            if target_value == patch_value {
                false
            } else {
                *target_value = patch_value.clone();
                true
            }
        }
    }
}

pub(crate) fn log_prompt_cache_trace(
    app_type: &AppType,
    provider: &Provider,
    endpoint: &str,
    api_format: Option<&str>,
    body: &Value,
    session_client_provided: bool,
) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }

    let prompt_cache_key = body
        .get("prompt_cache_key")
        .and_then(|value| value.as_str())
        .map(|key| format!("present(len={})", key.len()))
        .unwrap_or_else(|| "absent".to_string());
    let store = body
        .get("store")
        .map(value_for_log)
        .unwrap_or_else(|| "absent".to_string());
    let stream = body
        .get("stream")
        .map(value_for_log)
        .unwrap_or_else(|| "absent".to_string());
    let cache_controls = cache_control_summary(body);

    log::debug!(
        "[CacheTrace] app={}, provider={}, endpoint={}, api_format={}, session_client_provided={}, prompt_cache_key={}, store={}, stream={}, instructions_hash={}, system_hash={}, tools_hash={}, input_hash={}, messages_hash={}, include_hash={}, cache_controls={}, body_hash={}",
        app_type.as_str(),
        provider.id,
        endpoint,
        api_format.unwrap_or("native"),
        session_client_provided,
        prompt_cache_key,
        store,
        stream,
        short_value_hash(body.get("instructions")),
        short_value_hash(body.get("system")),
        short_value_hash(body.get("tools")),
        short_value_hash(body.get("input")),
        short_value_hash(body.get("messages")),
        short_value_hash(body.get("include")),
        cache_controls,
        short_value_hash(Some(body)),
    );
}

fn cache_control_summary(value: &Value) -> String {
    fn walk(value: &Value, count: &mut usize, ttls: &mut std::collections::BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                if let Some(cache_control) = object.get("cache_control") {
                    *count += 1;
                    let ttl = cache_control
                        .get("ttl")
                        .and_then(Value::as_str)
                        .unwrap_or("default");
                    ttls.insert(ttl.to_string());
                }
                for child in object.values() {
                    walk(child, count, ttls);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child, count, ttls);
                }
            }
            _ => {}
        }
    }

    let mut count = 0;
    let mut ttls = std::collections::BTreeSet::new();
    walk(value, &mut count, &mut ttls);
    format!(
        "count={count},ttls={}",
        if ttls.is_empty() {
            "none".to_string()
        } else {
            ttls.into_iter().collect::<Vec<_>>().join("|")
        }
    )
}

fn value_for_log(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Null => "null".to_string(),
        Value::Array(values) => format!("array(len={})", values.len()),
        Value::Object(values) => format!("object(len={})", values.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn private_keys_are_stripped_only_at_the_top_level_and_in_metadata() {
        let body = json!({
            "_debug": true,
            "metadata": {"_trace": "drop", "user_id": "u"},
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "mcp", "input": {"_meta": {"k": 1}, "_id": "x"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "ok", "_id": 3}]}
                ]}
            ],
            "tools": [{"name": "mcp", "input_schema": {"properties": {"_id": {"type": "string", "_note": "kept"}}}}]
        });
        let prepared = prepare_upstream_request_body(body);
        assert!(prepared.get("_debug").is_none());
        assert!(prepared["metadata"].get("_trace").is_none());
        assert_eq!(prepared["metadata"]["user_id"], "u");
        assert_eq!(
            prepared["messages"][0]["content"][0]["input"],
            json!({"_id": "x", "_meta": {"k": 1}})
        );
        assert_eq!(
            prepared["messages"][1]["content"][0]["content"][0]["_id"],
            3
        );
        assert_eq!(
            prepared["tools"][0]["input_schema"]["properties"]["_id"]["_note"],
            "kept"
        );
    }

    #[test]
    fn gemini_function_args_and_responses_items_keep_underscore_keys() {
        let gemini = json!({
            "contents": [{"role": "model", "parts": [{"functionCall": {"name": "f", "args": {"_id": 1}}}]}]
        });
        assert_eq!(
            prepare_upstream_request_body(gemini)["contents"][0]["parts"][0]["functionCall"]
                ["args"]["_id"],
            1
        );
        let responses = json!({
            "input": [{"type": "function_call", "call_id": "c", "arguments": "{\"_id\":1}", "_x": {"_y": 1}}]
        });
        let prepared = prepare_upstream_request_body(responses);
        assert_eq!(prepared["input"][0]["_x"]["_y"], 1);
    }

    #[test]
    fn copilot_session_id_priority() {
        let headers = http::HeaderMap::new();
        let body = json!({"metadata": {"session_id": "s1", "user_id": "raw"}});
        assert_eq!(copilot_session_id(&body, &headers), "s1");
        let body = json!({"metadata": {"user_id": "raw"}});
        assert_eq!(copilot_session_id(&body, &headers), "raw");
        let mut headers = http::HeaderMap::new();
        headers.insert("x-session-id", "h1".parse().unwrap());
        assert_eq!(copilot_session_id(&json!({}), &headers), "h1");
    }

    #[test]
    fn claude_code_system_prompt_is_prepended_once() {
        let mut body = json!({"system": "be brief"});
        prepend_claude_code_system_prompt(&mut body);
        prepend_claude_code_system_prompt(&mut body);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 2);
        assert_eq!(system[0]["text"], CLAUDE_CODE_SYSTEM_IDENTITY);
    }
}
