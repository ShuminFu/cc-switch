use std::fs;
use std::path::PathBuf;

use crate::config::atomic_write;
use crate::error::AppError;

const CLAUDE_DIR: &str = ".claude";
const CLAUDE_CONFIG_FILE: &str = "config.json";

fn claude_dir() -> Result<PathBuf, AppError> {
    // 优先使用设置中的覆盖目录
    if let Some(dir) = crate::settings::get_claude_override_dir() {
        return Ok(dir);
    }
    let home = dirs::home_dir().ok_or_else(|| AppError::Config("无法获取用户主目录".into()))?;
    Ok(home.join(CLAUDE_DIR))
}

pub fn claude_config_path() -> Result<PathBuf, AppError> {
    Ok(claude_dir()?.join(CLAUDE_CONFIG_FILE))
}

pub fn ensure_claude_dir_exists() -> Result<PathBuf, AppError> {
    let dir = claude_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    }
    Ok(dir)
}

pub fn read_claude_config() -> Result<Option<String>, AppError> {
    let path = claude_config_path()?;
    if path.exists() {
        let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
        Ok(Some(content))
    } else {
        Ok(None)
    }
}

fn is_managed_config(content: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(value) => value
            .get("primaryApiKey")
            .and_then(|v| v.as_str())
            .map(|val| val == "any")
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// 计算写入 `primaryApiKey = "any"` 后的新文件内容；无需改动时返回 `None`。
///
/// 现有内容无法解析为 JSON 对象时返回错误，而不是用 `{}` 覆盖用户文件，
/// 避免丢失其中的其它配置。
fn merge_primary_api_key(existing: Option<&str>) -> Result<Option<String>, AppError> {
    let mut map = match existing {
        Some(content) if !content.trim().is_empty() => {
            match serde_json::from_str::<serde_json::Value>(content) {
                Ok(serde_json::Value::Object(map)) => map,
                Ok(_) => {
                    return Err(AppError::Config(
                        "Claude config.json 根节点必须是 JSON 对象，已拒绝覆盖".into(),
                    ))
                }
                Err(e) => {
                    return Err(AppError::Config(format!(
                        "无法解析 Claude config.json，已拒绝覆盖: {e}"
                    )))
                }
            }
        }
        _ => serde_json::Map::new(),
    };

    let cur = map
        .get("primaryApiKey")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if cur == "any" && existing.is_some() {
        return Ok(None);
    }

    map.insert(
        "primaryApiKey".to_string(),
        serde_json::Value::String("any".to_string()),
    );
    let serialized = serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    Ok(Some(format!("{serialized}\n")))
}

pub fn write_claude_config() -> Result<bool, AppError> {
    // 增量写入：仅设置 primaryApiKey = "any"，保留其它字段
    let path = claude_config_path()?;
    ensure_claude_dir_exists()?;

    let existing = read_claude_config()?;
    match merge_primary_api_key(existing.as_deref())? {
        Some(content) => {
            atomic_write(&path, content.as_bytes())?;
            Ok(true)
        }
        None => Ok(false),
    }
}

pub fn clear_claude_config() -> Result<bool, AppError> {
    let path = claude_config_path()?;
    if !path.exists() {
        return Ok(false);
    }

    let content = match read_claude_config()? {
        Some(content) => content,
        None => return Ok(false),
    };

    let mut value = match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };

    let obj = match value.as_object_mut() {
        Some(obj) => obj,
        None => return Ok(false),
    };

    if obj.remove("primaryApiKey").is_none() {
        return Ok(false);
    }

    let serialized =
        serde_json::to_string_pretty(&value).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&path, format!("{serialized}\n").as_bytes())?;
    Ok(true)
}

pub fn claude_config_status() -> Result<(bool, PathBuf), AppError> {
    let path = claude_config_path()?;
    Ok((path.exists(), path))
}

pub fn is_claude_config_applied() -> Result<bool, AppError> {
    match read_claude_config()? {
        Some(content) => Ok(is_managed_config(&content)),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_creates_new_config_when_missing() {
        let out = merge_primary_api_key(None).unwrap().expect("should write");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v, serde_json::json!({ "primaryApiKey": "any" }));
    }

    #[test]
    fn merge_preserves_other_fields() {
        let out = merge_primary_api_key(Some(r#"{"foo":1,"primaryApiKey":"sk-x"}"#))
            .unwrap()
            .expect("should write");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v, serde_json::json!({ "foo": 1, "primaryApiKey": "any" }));
    }

    #[test]
    fn merge_is_noop_when_already_applied() {
        assert!(merge_primary_api_key(Some(r#"{"primaryApiKey":"any"}"#))
            .unwrap()
            .is_none());
    }

    #[test]
    fn merge_treats_empty_file_as_empty_object() {
        assert!(merge_primary_api_key(Some("  \n")).unwrap().is_some());
    }

    #[test]
    fn merge_refuses_to_clobber_unparseable_or_non_object_config() {
        assert!(merge_primary_api_key(Some("{ \"foo\": 1, // comment\n}")).is_err());
        assert!(merge_primary_api_key(Some("[1, 2]")).is_err());
    }
}
