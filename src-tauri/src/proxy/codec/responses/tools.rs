//! Codex 工具上下文：Responses 工具（function / namespace / custom / tool_search）与
//! 上游扁平函数名之间的映射
//!
//! Chat 与 Anthropic 上游只认识扁平的函数工具：namespace 子工具展开为
//! `namespace__name`（超长时截断并加哈希后缀），custom 工具变成只有一个 `input`
//! 字符串参数的函数工具，tool_search 变成固定的 `tool_search` 函数。回复中的工具调用
//! 再按同一张表还原为 Codex 认识的条目（`function_call` 带 namespace、
//! `custom_tool_call`、`tool_search_call`）。

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use crate::proxy::json_canonical::{canonical_json_string, short_sha256_hex};
use crate::proxy::providers::codex_chat_common::{
    response_function_call_item, response_function_call_item_with_namespace,
};

pub(crate) const TOOL_SEARCH_PROXY_NAME: &str = "tool_search";
pub(crate) const CUSTOM_TOOL_INPUT_FIELD: &str = "input";
pub(crate) const CHAT_TOOL_NAME_MAX_LEN: usize = 64;
pub(crate) const CUSTOM_TOOL_INPUT_DESCRIPTION: &str = "Raw string input for the original custom tool. Preserve formatting exactly and follow the original tool definition embedded in the description.";
pub(crate) const CUSTOM_TOOL_PRESERVED_METADATA_HEADING: &str = "Original tool definition:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodexToolKind {
    Function,
    Namespace,
    Custom,
    ToolSearch,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexToolSpec {
    pub(crate) kind: CodexToolKind,
    pub(crate) name: String,
    pub(crate) namespace: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CodexToolContext {
    chat_tools: Vec<Value>,
    seen_chat_names: HashSet<String>,
    chat_name_to_spec: HashMap<String, CodexToolSpec>,
    namespace_name_to_chat_name: HashMap<(String, String), String>,
}

impl CodexToolContext {
    pub(crate) fn chat_tools(&self) -> &[Value] {
        &self.chat_tools
    }

    pub(crate) fn lookup_chat_name(&self, chat_name: &str) -> Option<&CodexToolSpec> {
        self.chat_name_to_spec.get(chat_name)
    }

    pub(crate) fn is_custom_tool_chat_name(&self, chat_name: &str) -> bool {
        self.lookup_chat_name(chat_name)
            .is_some_and(|spec| matches!(&spec.kind, CodexToolKind::Custom))
    }

    pub(crate) fn chat_name_for_response_function(
        &self,
        name: &str,
        namespace: Option<&str>,
    ) -> String {
        if let Some(namespace) = namespace.filter(|value| !value.is_empty()) {
            if let Some(chat_name) = self
                .namespace_name_to_chat_name
                .get(&(namespace.to_string(), name.to_string()))
            {
                return chat_name.clone();
            }
            return flatten_namespace_tool_name(namespace, name);
        }

        name.to_string()
    }

    fn add_chat_tool(&mut self, chat_name: String, spec: CodexToolSpec, chat_tool: Value) {
        if chat_name.trim().is_empty() || self.seen_chat_names.contains(&chat_name) {
            return;
        }
        self.seen_chat_names.insert(chat_name.clone());
        if let Some(namespace) = spec.namespace.as_ref() {
            self.namespace_name_to_chat_name
                .insert((namespace.clone(), spec.name.clone()), chat_name.clone());
        }
        self.chat_name_to_spec.insert(chat_name, spec);
        self.chat_tools.push(chat_tool);
    }

    fn add_function_tool(&mut self, tool: &Value, namespace: Option<&str>) {
        let Some(original_name) = responses_tool_name(tool) else {
            return;
        };
        let chat_name = namespace
            .map(|namespace| flatten_namespace_tool_name(namespace, &original_name))
            .unwrap_or_else(|| original_name.clone());

        let Some(chat_tool) = responses_function_tool_to_chat_tool(tool, &chat_name) else {
            return;
        };
        let spec = CodexToolSpec {
            kind: if namespace.is_some() {
                CodexToolKind::Namespace
            } else {
                CodexToolKind::Function
            },
            name: original_name,
            namespace: namespace.map(ToString::to_string),
        };
        self.add_chat_tool(chat_name, spec, chat_tool);
    }

    fn add_custom_tool(&mut self, tool: &Value) {
        let Some(name) = responses_tool_name(tool) else {
            return;
        };
        let description = json!(responses_custom_tool_description(tool));
        let chat_tool = json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": {
                        CUSTOM_TOOL_INPUT_FIELD: {
                            "type": "string",
                            "description": CUSTOM_TOOL_INPUT_DESCRIPTION
                        }
                    },
                    "required": [CUSTOM_TOOL_INPUT_FIELD]
                }
            }
        });
        let spec = CodexToolSpec {
            kind: CodexToolKind::Custom,
            name: name.clone(),
            namespace: None,
        };
        self.add_chat_tool(name, spec, chat_tool);
    }

    fn add_tool_search_tool(&mut self) {
        let chat_tool = json!({
            "type": "function",
            "function": {
                "name": TOOL_SEARCH_PROXY_NAME,
                "description": "Search and load Codex tools, plugins, connectors, and MCP namespaces for the current task.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query for tools or connectors to load."
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of tool groups to return."
                        }
                    },
                    "required": ["query"]
                }
            }
        });
        let spec = CodexToolSpec {
            kind: CodexToolKind::ToolSearch,
            name: TOOL_SEARCH_PROXY_NAME.to_string(),
            namespace: None,
        };
        self.add_chat_tool(TOOL_SEARCH_PROXY_NAME.to_string(), spec, chat_tool);
    }

    fn add_namespace_tool(&mut self, namespace_tool: &Value) {
        let Some(namespace) = namespace_tool.get("name").and_then(|v| v.as_str()) else {
            return;
        };
        let Some(children) = namespace_tool
            .get("tools")
            .or_else(|| namespace_tool.get("children"))
            .and_then(|v| v.as_array())
        else {
            return;
        };

        for child in children {
            if child.get("type").and_then(|v| v.as_str()) == Some("function") {
                self.add_function_tool(child, Some(namespace));
            }
        }
    }

    fn add_response_tool(&mut self, tool: &Value) {
        match tool {
            Value::String(name) => {
                self.add_custom_tool(&json!({
                    "type": "custom",
                    "name": name
                }));
            }
            Value::Object(_) => match tool.get("type").and_then(|v| v.as_str()) {
                Some("function") => self.add_function_tool(tool, None),
                Some("custom") => self.add_custom_tool(tool),
                Some("tool_search") => self.add_tool_search_tool(),
                Some("namespace") => self.add_namespace_tool(tool),
                _ => {}
            },
            _ => {}
        }
    }
}

pub(crate) fn build_codex_tool_context_from_request(body: &Value) -> CodexToolContext {
    let mut context = CodexToolContext::default();

    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        for tool in tools {
            context.add_response_tool(tool);
        }
    }

    if let Some(input) = body.get("input") {
        collect_tool_search_output_tools(input, &mut context);
    }

    context
}

pub(crate) fn collect_tool_search_output_tools(value: &Value, context: &mut CodexToolContext) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_tool_search_output_tools(item, context);
            }
        }
        Value::Object(obj) => {
            if obj.get("type").and_then(|v| v.as_str()) == Some("tool_search_output") {
                if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
                    for tool in tools {
                        context.add_response_tool(tool);
                    }
                }
            }
            for value in obj.values() {
                collect_tool_search_output_tools(value, context);
            }
        }
        _ => {}
    }
}

pub(crate) fn flatten_namespace_tool_name(namespace: &str, name: &str) -> String {
    let full_name = format!("{namespace}__{name}");
    if full_name.len() <= CHAT_TOOL_NAME_MAX_LEN {
        return full_name;
    }

    let hash = short_sha256_hex(full_name.as_bytes());
    let suffix = format!("__{hash}");
    let prefix_len = CHAT_TOOL_NAME_MAX_LEN.saturating_sub(suffix.len());
    let mut prefix = String::new();
    for ch in full_name.chars() {
        if prefix.len() + ch.len_utf8() > prefix_len {
            break;
        }
        prefix.push(ch);
    }
    format!("{prefix}{suffix}")
}

pub(crate) fn responses_tool_name(tool: &Value) -> Option<String> {
    tool.get("function")
        .and_then(|function| function.get("name"))
        .or_else(|| tool.get("name"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn responses_custom_tool_description(tool: &Value) -> String {
    let mut description = String::new();
    description.push_str(CUSTOM_TOOL_PRESERVED_METADATA_HEADING);
    description.push_str("\n```json\n");
    description.push_str(&serialize_tool_definition_for_description(tool));
    description.push_str("\n```");
    description
}

pub(crate) fn serialize_tool_definition_for_description(tool: &Value) -> String {
    // Keep the embedded definition compact to reduce tool-description token
    // overhead for chat-only upstreams, while remaining stable across map
    // storage order.
    canonical_json_string(tool)
}

/// Normalize a function's `parameters` JSON Schema so `type` is always `"object"`.
///
/// Some Responses tools carry `parameters: null` or `parameters: {"type": null}`,
/// but OpenAI Chat Completions strictly requires `{"type": "object", "properties": {...}}`.
pub(crate) fn normalize_function_parameters(params: Option<&Value>) -> Value {
    let mut params = match params {
        Some(Value::Object(obj)) => Value::Object(obj.clone()),
        _ => json!({"type": "object", "properties": {}}),
    };
    if let Some(obj) = params.as_object_mut() {
        match obj.get("type").and_then(|v| v.as_str()) {
            Some("object") => {}
            _ => {
                obj.insert("type".to_string(), json!("object"));
            }
        }
    }
    params
}

pub(crate) fn responses_function_tool_to_chat_tool(tool: &Value, chat_name: &str) -> Option<Value> {
    if tool.get("type").and_then(|v| v.as_str()) != Some("function") {
        return None;
    }

    if let Some(function) = tool.get("function") {
        let mut chat_tool = json!({
            "type": "function",
            "function": function.clone()
        });
        if let Some(obj) = chat_tool
            .get_mut("function")
            .and_then(|value| value.as_object_mut())
        {
            // Ensure parameters.type is "object" for strict OpenAI-compatible providers
            if let Some(params) = obj.get("parameters") {
                let normalized = normalize_function_parameters(Some(params));
                if normalized != *params {
                    obj.insert("parameters".to_string(), normalized);
                }
            }

            obj.insert("name".to_string(), json!(chat_name));
            if let Some(strict) = tool.get("strict").cloned() {
                obj.entry("strict".to_string()).or_insert(strict);
            }
        }
        return Some(chat_tool);
    }

    let mut function = json!({
        "name": chat_name,
        "description": tool.get("description").cloned().unwrap_or(Value::Null),
        "parameters": normalize_function_parameters(tool.get("parameters"))
    });
    if let Some(strict) = tool.get("strict") {
        function["strict"] = strict.clone();
    }

    Some(json!({
        "type": "function",
        "function": function
    }))
}

pub(crate) fn response_tool_call_item_id_from_chat_name(
    call_id: &str,
    chat_name: &str,
    tool_context: &CodexToolContext,
) -> String {
    if tool_context.is_custom_tool_chat_name(chat_name) {
        format!("ctc_{call_id}")
    } else {
        format!("fc_{call_id}")
    }
}

pub(crate) fn response_tool_call_item_from_chat_name(
    item_id: &str,
    status: &str,
    call_id: &str,
    chat_name: &str,
    arguments: &str,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> Value {
    match tool_context.lookup_chat_name(chat_name) {
        Some(spec) if spec.kind == CodexToolKind::ToolSearch => {
            response_tool_search_call_item(call_id, status, arguments, reasoning)
        }
        Some(spec) if spec.kind == CodexToolKind::Custom => response_custom_tool_call_item(
            item_id, status, call_id, &spec.name, arguments, reasoning,
        ),
        Some(spec) => response_function_call_item_with_namespace(
            item_id,
            status,
            call_id,
            &spec.name,
            spec.namespace.as_deref(),
            arguments,
            reasoning,
        ),
        None => {
            response_function_call_item(item_id, status, call_id, chat_name, arguments, reasoning)
        }
    }
}

pub(crate) fn response_tool_search_call_item(
    call_id: &str,
    status: &str,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let parsed_arguments = parse_tool_arguments_object(arguments);
    let mut item = json!({
        "type": "tool_search_call",
        "call_id": call_id,
        "status": status,
        "execution": "client",
        "arguments": parsed_arguments
    });
    crate::proxy::providers::codex_chat_common::attach_optional_reasoning_content_field(
        &mut item, reasoning,
    );
    item
}

pub(crate) fn response_custom_tool_call_item(
    item_id: &str,
    status: &str,
    call_id: &str,
    name: &str,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let input = custom_tool_input_from_chat_arguments(arguments);
    let mut item = json!({
        "id": item_id,
        "type": "custom_tool_call",
        "status": status,
        "call_id": call_id,
        "name": name,
        "input": input
    });
    crate::proxy::providers::codex_chat_common::attach_optional_reasoning_content_field(
        &mut item, reasoning,
    );
    item
}

pub(crate) fn parse_tool_arguments_object(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str::<Value>(arguments)
        .ok()
        .filter(|value| value.is_object())
        .unwrap_or_else(|| json!({ "query": arguments }))
}

pub(crate) fn custom_tool_input_from_chat_arguments(arguments: &str) -> String {
    if arguments.trim().is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(obj)) => obj
            .get(CUSTOM_TOOL_INPUT_FIELD)
            .and_then(|value| value.as_str())
            .unwrap_or(arguments)
            .to_string(),
        _ => arguments.to_string(),
    }
}
