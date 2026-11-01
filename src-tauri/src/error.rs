use std::collections::BTreeMap;
use std::path::Path;
use std::sync::PoisonError;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("配置错误: {0}")]
    Config(String),
    #[error("无效输入: {0}")]
    InvalidInput(String),
    #[error("IO 错误: {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{context}: {source}")]
    IoContext {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("JSON 解析错误: {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("JSON 序列化失败: {source}")]
    JsonSerialize {
        #[source]
        source: serde_json::Error,
    },
    #[error("TOML 解析错误: {path}: {source}")]
    Toml {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("锁获取失败: {0}")]
    Lock(String),
    #[error("MCP 校验失败: {0}")]
    McpValidation(String),
    #[error("{0}")]
    Message(String),
    #[error("HTTP {status}: {body}")]
    HttpStatus { status: u16, body: String },
    #[error("{zh} ({en})")]
    Localized {
        key: &'static str,
        zh: String,
        en: String,
    },
    #[error("数据库错误: {0}")]
    Database(String),
    #[error("OMO 配置文件不存在")]
    OmoConfigNotFound,
    #[error("所有供应商已熔断，无可用渠道")]
    AllProvidersCircuitOpen,
    #[error("未配置供应商")]
    NoProvidersConfigured,
}

impl AppError {
    pub fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    pub fn json(path: impl AsRef<Path>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    pub fn toml(path: impl AsRef<Path>, source: toml::de::Error) -> Self {
        Self::Toml {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    pub fn localized(key: &'static str, zh: impl Into<String>, en: impl Into<String>) -> Self {
        Self::Localized {
            key,
            zh: zh.into(),
            en: en.into(),
        }
    }
}

impl<T> From<PoisonError<T>> for AppError {
    fn from(err: PoisonError<T>) -> Self {
        Self::Lock(err.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Database(err.to_string())
    }
}

impl From<AppError> for String {
    fn from(err: AppError) -> Self {
        err.to_string()
    }
}

/// Wire shape of an [`AppError`] crossing the Tauri IPC boundary (CMD-12).
///
/// - `code` is a stable i18n key (`errors.config`, `sync.conflict.remote_changed`, ...)
///   that the frontend translates with `t(code, params)`.
/// - `message` is the legacy zh/en text (the `Display` output), kept as the fallback
///   for keys without a translation and for logs.
/// - `params` carries interpolation values; for [`AppError::Localized`] it holds the
///   `zh` and `en` fallbacks so the UI can pick the one matching its language.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AppErrorPayload {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<BTreeMap<String, String>>,
}

impl AppError {
    /// Stable, i18n-key-shaped identifier of this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Config(_) => "errors.config",
            Self::InvalidInput(_) => "errors.invalidInput",
            Self::Io { .. } => "errors.io",
            Self::IoContext { .. } => "errors.ioContext",
            Self::Json { .. } => "errors.json",
            Self::JsonSerialize { .. } => "errors.jsonSerialize",
            Self::Toml { .. } => "errors.toml",
            Self::Lock(_) => "errors.lock",
            Self::McpValidation(_) => "errors.mcpValidation",
            Self::Message(_) => "errors.message",
            Self::HttpStatus { .. } => "errors.httpStatus",
            Self::Localized { key, .. } => key,
            Self::Database(_) => "errors.database",
            Self::OmoConfigNotFound => "errors.omoConfigNotFound",
            Self::AllProvidersCircuitOpen => "errors.allProvidersCircuitOpen",
            Self::NoProvidersConfigured => "errors.noProvidersConfigured",
        }
    }

    /// Interpolation parameters for [`AppError::code`].
    pub fn params(&self) -> Option<BTreeMap<String, String>> {
        let pairs: Vec<(&str, String)> = match self {
            Self::Config(detail)
            | Self::InvalidInput(detail)
            | Self::Lock(detail)
            | Self::McpValidation(detail)
            | Self::Message(detail)
            | Self::Database(detail) => vec![("detail", detail.clone())],
            Self::Io { path, source } => {
                vec![("path", path.clone()), ("detail", source.to_string())]
            }
            Self::IoContext { context, source } => {
                vec![("context", context.clone()), ("detail", source.to_string())]
            }
            Self::Json { path, source } => {
                vec![("path", path.clone()), ("detail", source.to_string())]
            }
            Self::JsonSerialize { source } => vec![("detail", source.to_string())],
            Self::Toml { path, source } => {
                vec![("path", path.clone()), ("detail", source.to_string())]
            }
            Self::HttpStatus { status, body } => {
                vec![("status", status.to_string()), ("body", body.clone())]
            }
            Self::Localized { zh, en, .. } => vec![("zh", zh.clone()), ("en", en.clone())],
            Self::OmoConfigNotFound
            | Self::AllProvidersCircuitOpen
            | Self::NoProvidersConfigured => Vec::new(),
        };
        if pairs.is_empty() {
            None
        } else {
            Some(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
        }
    }

    pub fn to_payload(&self) -> AppErrorPayload {
        AppErrorPayload {
            code: self.code().to_string(),
            message: self.to_string(),
            params: self.params(),
        }
    }
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_payload().serialize(serializer)
    }
}

/// Commands returning `Result<_, AppError>` are typed with the payload shape in
/// `src/lib/bindings.ts`.
impl specta::Type for AppError {
    fn inline(
        types: &mut specta::TypeCollection,
        generics: specta::Generics,
    ) -> specta::datatype::DataType {
        <AppErrorPayload as specta::Type>::inline(types, generics)
    }

    fn reference(
        types: &mut specta::TypeCollection,
        generics: &[specta::datatype::DataType],
    ) -> specta::datatype::reference::Reference {
        <AppErrorPayload as specta::Type>::reference(types, generics)
    }
}

/// 格式化为 JSON 错误字符串，前端可解析为结构化错误
pub fn format_skill_error(
    code: &str,
    context: &[(&str, &str)],
    suggestion: Option<&str>,
) -> String {
    use serde_json::json;

    let mut ctx_map = serde_json::Map::new();
    for (key, value) in context {
        ctx_map.insert(key.to_string(), json!(value));
    }

    let error_obj = json!({
        "code": code,
        "context": ctx_map,
        "suggestion": suggestion,
    });

    serde_json::to_string(&error_obj).unwrap_or_else(|_| {
        // 如果 JSON 序列化失败，返回简单格式
        format!("ERROR:{code}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localized_error_serializes_its_key_as_code() {
        let err = AppError::localized(
            "sync.conflict.remote_changed",
            "远端数据自上次同步后已被其他设备更新",
            "the remote data changed since the last sync",
        );
        let value = serde_json::to_value(&err).unwrap();
        assert_eq!(value["code"], "sync.conflict.remote_changed");
        assert_eq!(
            value["message"],
            "远端数据自上次同步后已被其他设备更新 (the remote data changed since the last sync)"
        );
        assert_eq!(
            value["params"]["en"],
            "the remote data changed since the last sync"
        );
        assert_eq!(
            value["params"]["zh"],
            "远端数据自上次同步后已被其他设备更新"
        );
    }

    #[test]
    fn plain_variants_serialize_code_message_and_detail() {
        let value = serde_json::to_value(AppError::InvalidInput("bad".into())).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "code": "errors.invalidInput",
                "message": "无效输入: bad",
                "params": { "detail": "bad" }
            })
        );
    }

    #[test]
    fn unit_variants_omit_params() {
        let value = serde_json::to_value(AppError::NoProvidersConfigured).unwrap();
        assert_eq!(value["code"], "errors.noProvidersConfigured");
        assert!(value.get("params").is_none());
    }

    #[test]
    fn string_conversion_keeps_the_legacy_display_text() {
        let s: String = AppError::Config("x".into()).into();
        assert_eq!(s, "配置错误: x");
    }
}
