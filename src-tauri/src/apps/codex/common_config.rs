//! Codex 通用配置片段：对 `config.toml` 做结构化（toml_edit）合并 / 判定 / 剥离

use serde_json::Value;
use toml_edit::{DocumentMut, Item, TableLike};

use super::config::{CODEX_WEB_SEARCH_DISABLED, CODEX_WEB_SEARCH_FIELD};
use crate::error::AppError;

fn toml_value_is_subset(target: &toml_edit::Value, source: &toml_edit::Value) -> bool {
    match (target, source) {
        (toml_edit::Value::String(target), toml_edit::Value::String(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Integer(target), toml_edit::Value::Integer(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Float(target), toml_edit::Value::Float(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Boolean(target), toml_edit::Value::Boolean(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Datetime(target), toml_edit::Value::Datetime(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Array(target), toml_edit::Value::Array(source)) => {
            toml_array_contains_subset(target, source)
        }
        (toml_edit::Value::InlineTable(target), toml_edit::Value::InlineTable(source)) => {
            source.iter().all(|(key, source_item)| {
                target
                    .get(key)
                    .is_some_and(|target_item| toml_value_is_subset(target_item, source_item))
            })
        }
        _ => false,
    }
}

fn toml_array_contains_subset(target: &toml_edit::Array, source: &toml_edit::Array) -> bool {
    let mut matched = vec![false; target.len()];
    let target_items: Vec<&toml_edit::Value> = target.iter().collect();

    source.iter().all(|source_item| {
        if let Some((index, _)) = target_items
            .iter()
            .enumerate()
            .find(|(index, target_item)| {
                !matched[*index] && toml_value_is_subset(target_item, source_item)
            })
        {
            matched[index] = true;
            true
        } else {
            false
        }
    })
}

fn toml_remove_array_items(target: &mut toml_edit::Array, source: &toml_edit::Array) {
    for source_item in source.iter() {
        let index = {
            let target_items: Vec<&toml_edit::Value> = target.iter().collect();
            target_items
                .iter()
                .enumerate()
                .find(|(_, target_item)| toml_value_is_subset(target_item, source_item))
                .map(|(index, _)| index)
        };

        if let Some(index) = index {
            target.remove(index);
        }
    }
}

fn toml_item_is_subset(target: &Item, source: &Item) -> bool {
    if let Some(source_table) = source.as_table_like() {
        let Some(target_table) = target.as_table_like() else {
            return false;
        };
        return source_table.iter().all(|(key, source_item)| {
            target_table
                .get(key)
                .is_some_and(|target_item| toml_item_is_subset(target_item, source_item))
        });
    }

    match (target.as_value(), source.as_value()) {
        (Some(target_value), Some(source_value)) => {
            toml_value_is_subset(target_value, source_value)
        }
        _ => false,
    }
}

fn merge_toml_item(target: &mut Item, source: &Item) {
    if let Some(source_table) = source.as_table_like() {
        if let Some(target_table) = target.as_table_like_mut() {
            merge_toml_table_like(target_table, source_table);
            return;
        }
    }

    *target = source.clone();
}

fn merge_toml_table_like(target: &mut dyn TableLike, source: &dyn TableLike) {
    for (key, source_item) in source.iter() {
        match target.get_mut(key) {
            Some(target_item) => merge_toml_item(target_item, source_item),
            None => {
                target.insert(key, source_item.clone());
            }
        }
    }
}

fn remove_toml_item(target: &mut Item, source: &Item) {
    if let Some(source_table) = source.as_table_like() {
        if let Some(target_table) = target.as_table_like_mut() {
            remove_toml_table_like(target_table, source_table);
            if target_table.is_empty() {
                *target = Item::None;
            }
            return;
        }
    }

    if let Some(source_value) = source.as_value() {
        let mut remove_item = false;

        if let Some(target_value) = target.as_value_mut() {
            match (target_value, source_value) {
                (toml_edit::Value::Array(target_arr), toml_edit::Value::Array(source_arr)) => {
                    toml_remove_array_items(target_arr, source_arr);
                    remove_item = target_arr.is_empty();
                }
                (target_value, source_value)
                    if toml_value_is_subset(target_value, source_value) =>
                {
                    remove_item = true;
                }
                _ => {}
            }
        }

        if remove_item {
            *target = Item::None;
        }
    }
}

fn remove_toml_table_like(target: &mut dyn TableLike, source: &dyn TableLike) {
    let keys: Vec<String> = source.iter().map(|(key, _)| key.to_string()).collect();

    for key in keys {
        let mut remove_key = false;
        if let (Some(target_item), Some(source_item)) = (target.get_mut(&key), source.get(&key)) {
            remove_toml_item(target_item, source_item);
            remove_key = target_item.is_none()
                || target_item
                    .as_table_like()
                    .is_some_and(|table_like| table_like.is_empty());
        }

        if remove_key {
            target.remove(&key);
        }
    }
}

/// 前端表单勾选/取消"使用通用配置"时，对编辑器里的 config.toml 文本做
/// 结构化合并/剥离。必须在后端用 toml_edit 做：前端 smol-toml 只能
/// parse → merge → 整文档重序列化，注释全丢、键序重排，还会生成多余的
/// 空父表头（如 `[model_providers]`）。
pub fn update_toml_common_config_snippet(
    config_toml: &str,
    snippet_toml: &str,
    enabled: bool,
) -> Result<String, AppError> {
    let trimmed = snippet_toml.trim();
    if trimmed.is_empty() {
        return Ok(config_toml.to_string());
    }

    let mut target_doc = if config_toml.trim().is_empty() {
        DocumentMut::new()
    } else {
        config_toml
            .parse::<DocumentMut>()
            .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?
    };
    let source_doc = trimmed
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex common config snippet: {e}")))?;

    if enabled {
        merge_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
    } else {
        remove_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
    }

    Ok(target_doc.to_string())
}

/// 提取 Codex 的通用配置片段（TOML），剥掉供应商专属字段与 cc-switch 注入产物
pub(crate) fn extract_common_config(settings: &Value) -> Result<String, AppError> {
    // Codex config is stored as { "auth": {...}, "config": "toml string" }
    let config_toml = settings
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if config_toml.is_empty() {
        return Ok(String::new());
    }

    let mut doc = config_toml
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| AppError::Message(format!("TOML parse error: {e}")))?;

    // Remove provider-specific fields.
    let root = doc.as_table_mut();
    root.remove("model");
    root.remove("model_provider");
    // Legacy/alt formats might use a top-level base_url.
    root.remove("base_url");
    // wire_api 与 base_url 同属供应商路由语义：无 model_provider 时
    // update_codex_toml_field / 前端 setCodexWireApi 都会把它落在顶层，
    // 进了片段会改写其它供应商的协议选择（chat vs responses）。
    root.remove("wire_api");

    // Remove entire model_providers table (provider-specific configuration)
    root.remove("model_providers");

    // MCP 服务器归 DB mcp_servers 表所有：进了共享片段会绕过按应用的
    // 启用状态被合并进所有勾选通用配置的供应商，且在通用配置编辑框里
    // 显示为一份"重复"的 MCP 配置。
    root.remove("mcp_servers");
    // 历史错误格式 [mcp.servers] 一并剥离（与 strip_codex_mcp_servers_from_settings
    // 一致）：sync_all_enabled 只管理 [mcp_servers.*]，legacy 形态一旦进了
    // 片段就会被合并进所有供应商，且没有任何同步路径能清掉这个孤儿。
    if let Some(mcp_tbl) = root
        .get_mut("mcp")
        .and_then(|item| item.as_table_like_mut())
    {
        mcp_tbl.remove("servers");
        if mcp_tbl.is_empty() {
            root.remove("mcp");
        }
    }

    // cc-switch 写 live 时注入的产物一律不进共享片段：
    // - experimental_bearer_token 正常写在 [model_providers.<id>] 内（上面
    //   整表已剥），但无活跃路由 / 内建保留 id / 路由表缺失三种 fallback
    //   会落在顶层——不剥等于把 API 密钥写进共享片段。
    root.remove("experimental_bearer_token");
    // - model_catalog_json 指向按供应商生成的 catalog 投影文件（DB 为 SSOT）。
    root.remove("model_catalog_json");
    // - web_search 只剥 cc-switch 注入的 "disabled" 哨兵；用户手设的其它值
    //   属于可共享偏好，保留。
    if root
        .get(CODEX_WEB_SEARCH_FIELD)
        .and_then(|item| item.as_str())
        == Some(CODEX_WEB_SEARCH_DISABLED)
    {
        root.remove(CODEX_WEB_SEARCH_FIELD);
    }

    // Clean up multiple empty lines (keep at most one blank line).
    let mut cleaned = String::new();
    let mut blank_run = 0usize;
    for line in doc.to_string().lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                cleaned.push('\n');
            }
            continue;
        }
        blank_run = 0;
        cleaned.push_str(line);
        cleaned.push('\n');
    }

    Ok(cleaned.trim().to_string())
}

/// `settings.config` 的 TOML 是否已包含整个片段
pub(super) fn contains(settings: &Value, snippet: &str) -> bool {
    let config_toml = settings.get("config").and_then(Value::as_str).unwrap_or("");
    if config_toml.trim().is_empty() {
        return false;
    }

    let target_doc = match config_toml.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(_) => return false,
    };
    let source_doc = match snippet.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(_) => return false,
    };

    toml_item_is_subset(target_doc.as_item(), source_doc.as_item())
}

/// 把片段合并进 `settings.config`
pub(super) fn apply(settings: &Value, snippet: &str) -> Result<Value, AppError> {
    let mut result = settings.clone();
    let config_toml = settings.get("config").and_then(Value::as_str).unwrap_or("");
    let mut target_doc = if config_toml.trim().is_empty() {
        DocumentMut::new()
    } else {
        config_toml.parse::<DocumentMut>().map_err(|e| {
            AppError::Message(format!(
                "Invalid Codex config.toml while applying common config: {e}"
            ))
        })?
    };
    let source_doc = snippet
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex common config snippet: {e}")))?;

    merge_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
    if let Some(obj) = result.as_object_mut() {
        obj.insert("config".to_string(), Value::String(target_doc.to_string()));
    }
    Ok(result)
}

/// 从 `settings.config` 中剥掉片段
pub(super) fn remove(settings: &Value, snippet: &str) -> Result<Value, AppError> {
    let mut result = settings.clone();
    let config_toml = settings.get("config").and_then(Value::as_str).unwrap_or("");
    let mut target_doc = if config_toml.trim().is_empty() {
        DocumentMut::new()
    } else {
        config_toml.parse::<DocumentMut>().map_err(|e| {
            AppError::Message(format!(
                "Invalid Codex config.toml while removing common config: {e}"
            ))
        })?
    };
    let source_doc = snippet
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex common config snippet: {e}")))?;

    remove_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
    if let Some(obj) = result.as_object_mut() {
        obj.insert("config".to_string(), Value::String(target_doc.to_string()));
    }
    Ok(result)
}
