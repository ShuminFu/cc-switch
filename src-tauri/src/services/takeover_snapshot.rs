//! 代理接管前 live 文件的原始字节快照（CCS-152）
//!
//! 接管时 Live 备份以 JSON 形式保存（`proxy_live_backup.original_config`），恢复时
//! 再由各应用的写入器重新序列化：语义不变，但用户手写的缩进、注释与键顺序会丢失。
//! 这里在备份的同时记录 live 文件的原文，恢复时在写入器写完之后逐字节覆盖回去。
//!
//! - 快照存放在同一备份行的 `raw_snapshot` 列：备份被热切换等路径重写时整行替换、
//!   快照随之清空，此后恢复自动回退为只按备份 JSON 写入（与旧行为一致）。
//! - [`TakeoverEngine::Legacy`]（设置项 `takeoverEngine = "legacy"`）保留一个版本作为
//!   回退：既不记录也不使用快照，完全走原有的恢复路径。

use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::app_config::AppType;

/// 代理接管引擎
///
/// `Legacy` 保留一个版本作为回退：在接管整合（CCS-153 / CCS-154）出现回归时，
/// 让接管、备份与恢复退回原有路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TakeoverEngine {
    /// 原有路径：恢复时只按备份 JSON 重写 live
    Legacy,
    /// 当前路径：额外记录并逐字节还原接管前的 live 文件
    #[default]
    Unified,
}

impl TakeoverEngine {
    /// 设置中使用的字符串形式
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Unified => "unified",
        }
    }

    /// 宽松解析：未知值回退为默认的 [`TakeoverEngine::Unified`]，避免一个未知取值
    /// 让整个设置文件解析失败
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "legacy" => Self::Legacy,
            "unified" => Self::Unified,
            other => {
                log::warn!("[Takeover] Unknown takeoverEngine `{other}`, using unified");
                Self::Unified
            }
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// 是否走统一接管引擎（`takeover(app, Strict | BestEffort)`，CCS-154）
    pub fn uses_unified_takeover(self) -> bool {
        self == Self::Unified
    }

    /// 是否记录并使用原始字节快照
    pub fn uses_raw_snapshot(self) -> bool {
        self == Self::Unified
    }
}

impl Serialize for TakeoverEngine {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TakeoverEngine {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // 非字符串（null、数字等）同样回退为默认值，而不是让整个设置解析失败
        match Value::deserialize(deserializer)? {
            Value::String(value) => Ok(Self::from_setting(&value)),
            other => {
                log::warn!("[Takeover] Non-string takeoverEngine `{other}`, using unified");
                Ok(Self::default())
            }
        }
    }
}

/// 一个 live 文件的原文
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SnapshotFile {
    pub path: PathBuf,
    pub content: String,
}

/// 某个应用接管前的 live 文件原文（只含当时存在的文件）
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct RawLiveSnapshot {
    pub files: Vec<SnapshotFile>,
}

/// 接管会改写的 live 文件（见 [`crate::apps::adapter::ProxyTakeover::live_files`]）
pub(crate) fn takeover_live_files(app_type: &AppType) -> Vec<PathBuf> {
    crate::apps::adapter(app_type)
        .takeover()
        .map(|takeover| takeover.live_files())
        .unwrap_or_default()
}

impl RawLiveSnapshot {
    /// 读取该应用当前的 live 文件原文；有文件不是 UTF-8 或读取失败时放弃快照
    pub(crate) fn capture(app_type: &AppType) -> Option<Self> {
        let mut files = Vec::new();
        for path in takeover_live_files(app_type) {
            if !path.exists() {
                continue;
            }
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    log::warn!(
                        "读取 {} 原文失败，{app_type:?} 不记录接管快照: {e}",
                        path.display()
                    );
                    return None;
                }
            };
            let Ok(content) = String::from_utf8(bytes) else {
                log::warn!(
                    "{} 不是 UTF-8 文本，{app_type:?} 不记录接管快照",
                    path.display()
                );
                return None;
            };
            files.push(SnapshotFile { path, content });
        }
        if files.is_empty() {
            return None;
        }
        Some(Self { files })
    }

    pub(crate) fn to_json(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|e| format!("序列化接管快照失败: {e}"))
    }

    pub(crate) fn from_json(raw: &str) -> Result<Self, String> {
        serde_json::from_str(raw).map_err(|e| format!("解析接管快照失败: {e}"))
    }

    /// 把原文写回 live；只写当前仍属于该应用接管范围的路径
    ///（期间若改了配置目录覆盖，旧路径不再写入，保留写入器按备份写出的结果）
    pub(crate) fn restore(&self, app_type: &AppType) -> Result<(), String> {
        let current = takeover_live_files(app_type);
        for file in &self.files {
            if !current.contains(&file.path) {
                log::warn!(
                    "{app_type:?} 接管快照路径 {} 已不是当前 live 路径，跳过逐字节还原",
                    file.path.display()
                );
                continue;
            }
            crate::config::write_text_file(&file.path, &file.content)
                .map_err(|e| format!("还原 {} 原文失败: {e}", file.path.display()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_parses_leniently() {
        assert_eq!(
            TakeoverEngine::from_setting("legacy"),
            TakeoverEngine::Legacy
        );
        assert_eq!(
            TakeoverEngine::from_setting(" Legacy "),
            TakeoverEngine::Legacy
        );
        assert_eq!(
            TakeoverEngine::from_setting("unified"),
            TakeoverEngine::Unified
        );
        assert_eq!(
            TakeoverEngine::from_setting("nope"),
            TakeoverEngine::Unified
        );
        assert_eq!(TakeoverEngine::default(), TakeoverEngine::Unified);
        assert!(!TakeoverEngine::Legacy.uses_raw_snapshot());
        assert!(TakeoverEngine::Unified.uses_raw_snapshot());
    }

    #[test]
    fn engine_serde_shape() {
        assert_eq!(
            serde_json::to_value(TakeoverEngine::Legacy).unwrap(),
            Value::String("legacy".into())
        );
        let parsed: TakeoverEngine = serde_json::from_value(serde_json::json!(42)).unwrap();
        assert_eq!(parsed, TakeoverEngine::Unified);
        let parsed: TakeoverEngine = serde_json::from_value(serde_json::json!("legacy")).unwrap();
        assert_eq!(parsed, TakeoverEngine::Legacy);
    }

    #[test]
    fn snapshot_json_round_trip() {
        let snapshot = RawLiveSnapshot {
            files: vec![SnapshotFile {
                path: PathBuf::from("/tmp/settings.json"),
                content: "{\n    \"env\": {}\n}\n".to_string(),
            }],
        };
        let json = snapshot.to_json().unwrap();
        assert_eq!(RawLiveSnapshot::from_json(&json).unwrap(), snapshot);
    }
}
