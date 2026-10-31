use std::fs;
use std::path::PathBuf;

use crate::adapters::live_file::{update_json_file, WriteOptions};
use crate::error::AppError;

const CLAUDE_CONFIG_FILE: &str = "config.json";

fn claude_dir() -> Result<PathBuf, AppError> {
    // 与其余 Claude 配置共用目录解析（设置中的覆盖目录 → ~/.claude），CFG-32
    Ok(crate::config::get_claude_config_dir())
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

/// Root of `~/.claude/config.json` as an object; anything else is an error so
/// the file is never replaced by `{}` (CFG-48).
fn config_object(
    root: &mut serde_json::Value,
) -> Result<&mut serde_json::Map<String, serde_json::Value>, AppError> {
    root.as_object_mut()
        .ok_or_else(|| AppError::Config("~/.claude/config.json 根必须是 JSON 对象".into()))
}

pub fn write_claude_config() -> Result<bool, AppError> {
    // 增量写入：仅设置 primaryApiKey = "any"，保留其它字段；无法解析时报错且不改动文件
    let path = claude_config_path()?;
    ensure_claude_dir_exists()?;

    let outcome = update_json_file(&path, WriteOptions::backup("claude"), |root| {
        let map = config_object(root)?;
        if map.get("primaryApiKey").and_then(|v| v.as_str()) != Some("any") {
            map.insert(
                "primaryApiKey".to_string(),
                serde_json::Value::String("any".to_string()),
            );
        }
        Ok(())
    })?;
    Ok(outcome.written())
}

pub fn clear_claude_config() -> Result<bool, AppError> {
    let path = claude_config_path()?;
    if !path.exists() {
        return Ok(false);
    }

    let outcome = update_json_file(&path, WriteOptions::backup("claude"), |root| {
        config_object(root)?.shift_remove("primaryApiKey");
        Ok(())
    })?;
    Ok(outcome.written())
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
    use serial_test::serial;

    struct HomeGuard(Option<String>);
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    #[serial]
    fn invalid_config_json_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = HomeGuard(std::env::var("CC_SWITCH_TEST_HOME").ok());
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        let path = claude_config_path().unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        fs::write(&path, "{ \"a\": 1, }").unwrap();
        assert!(write_claude_config().is_err());
        assert!(clear_claude_config().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ \"a\": 1, }");

        fs::write(&path, "[1]").unwrap();
        assert!(write_claude_config().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[1]");

        fs::write(&path, "{\n  \"keep\": true\n}\n").unwrap();
        assert!(write_claude_config().unwrap());
        assert!(!write_claude_config().unwrap());
        assert!(is_claude_config_applied().unwrap());
        assert!(clear_claude_config().unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\n  \"keep\": true\n}\n"
        );
    }
}
