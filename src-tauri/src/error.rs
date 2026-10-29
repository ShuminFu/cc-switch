use std::path::Path;
use std::sync::PoisonError;

use serde::{Deserialize, Serialize};
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
    /// 应用不具备某项能力（如 Claude Desktop 没有提示词文件）
    #[error("{app} 暂不支持 {feature} ({app} does not support {feature})")]
    Unsupported {
        app: &'static str,
        feature: &'static str,
    },
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

    pub fn unsupported(app: &crate::app_config::AppType, feature: &'static str) -> Self {
        Self::Unsupported {
            app: app.descriptor().display_name,
            feature,
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

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// IPC 错误码：前端按 `code` 分支，不再匹配错误文案。
///
/// 序列化为 SCREAMING_SNAKE_CASE 字符串，新增变体时同步更新 `ALL`
/// 与前端 `src/lib/api/ipc.ts` 中的 `IpcErrorCode`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// 未分类错误（含尚未迁移的 String 错误）
    Unknown,
    Config,
    InvalidInput,
    Io,
    Json,
    Toml,
    Lock,
    McpValidation,
    HttpStatus,
    /// 带 i18n key 的本地化错误，key 见 `IpcError::i18n_key`
    Localized,
    Database,
    OmoConfigNotFound,
    /// 所有供应商已熔断
    CircuitOpen,
    NoProvidersConfigured,
    Unsupported,
    // ---- Skills ----
    SkillNotFound,
    InvalidSkillDirectory,
    SkillDirectoryConflict,
    SkillDirNotFound,
    DownloadTimeout,
    DownloadFailed,
    EmptyArchive,
    NoSkillsInZip,
    // ---- 可操作错误（前端 toast 提供修复按钮） ----
    /// 所需 CLI 工具未安装，`details = { tool }`
    ToolNotInstalled,
    /// 代理接管冲突，`details = { appType }`
    TakeoverActive,
    /// 数据库版本高于应用支持版本，`details = { backupId? }`
    DbVersionTooNew,
}

impl ErrorCode {
    pub const ALL: &'static [ErrorCode] = &[
        ErrorCode::Unknown,
        ErrorCode::Config,
        ErrorCode::InvalidInput,
        ErrorCode::Io,
        ErrorCode::Json,
        ErrorCode::Toml,
        ErrorCode::Lock,
        ErrorCode::McpValidation,
        ErrorCode::HttpStatus,
        ErrorCode::Localized,
        ErrorCode::Database,
        ErrorCode::OmoConfigNotFound,
        ErrorCode::CircuitOpen,
        ErrorCode::NoProvidersConfigured,
        ErrorCode::Unsupported,
        ErrorCode::SkillNotFound,
        ErrorCode::InvalidSkillDirectory,
        ErrorCode::SkillDirectoryConflict,
        ErrorCode::SkillDirNotFound,
        ErrorCode::DownloadTimeout,
        ErrorCode::DownloadFailed,
        ErrorCode::EmptyArchive,
        ErrorCode::NoSkillsInZip,
        ErrorCode::ToolNotInstalled,
        ErrorCode::TakeoverActive,
        ErrorCode::DbVersionTooNew,
    ];

    /// 稳定的线上字符串，与 serde 序列化结果一致
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Unknown => "UNKNOWN",
            ErrorCode::Config => "CONFIG",
            ErrorCode::InvalidInput => "INVALID_INPUT",
            ErrorCode::Io => "IO",
            ErrorCode::Json => "JSON",
            ErrorCode::Toml => "TOML",
            ErrorCode::Lock => "LOCK",
            ErrorCode::McpValidation => "MCP_VALIDATION",
            ErrorCode::HttpStatus => "HTTP_STATUS",
            ErrorCode::Localized => "LOCALIZED",
            ErrorCode::Database => "DATABASE",
            ErrorCode::OmoConfigNotFound => "OMO_CONFIG_NOT_FOUND",
            ErrorCode::CircuitOpen => "CIRCUIT_OPEN",
            ErrorCode::NoProvidersConfigured => "NO_PROVIDERS_CONFIGURED",
            ErrorCode::Unsupported => "UNSUPPORTED",
            ErrorCode::SkillNotFound => "SKILL_NOT_FOUND",
            ErrorCode::InvalidSkillDirectory => "INVALID_SKILL_DIRECTORY",
            ErrorCode::SkillDirectoryConflict => "SKILL_DIRECTORY_CONFLICT",
            ErrorCode::SkillDirNotFound => "SKILL_DIR_NOT_FOUND",
            ErrorCode::DownloadTimeout => "DOWNLOAD_TIMEOUT",
            ErrorCode::DownloadFailed => "DOWNLOAD_FAILED",
            ErrorCode::EmptyArchive => "EMPTY_ARCHIVE",
            ErrorCode::NoSkillsInZip => "NO_SKILLS_IN_ZIP",
            ErrorCode::ToolNotInstalled => "TOOL_NOT_INSTALLED",
            ErrorCode::TakeoverActive => "TAKEOVER_ACTIVE",
            ErrorCode::DbVersionTooNew => "DB_VERSION_TOO_NEW",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Tauri 命令返回给前端的结构化错误
///
/// 线上形态：`{ code, message, details, i18nKey }`。`message` 保持原有的
/// 人类可读文案，`details` 携带插值参数，`i18nKey` 来自 `AppError::Localized`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    pub code: ErrorCode,
    pub message: String,
    /// 装箱以控制 `Result<T, IpcError>` 的体积（clippy::result_large_err）
    // specta 对 serde_json::Value 内联展开会无限递归，导出为 unknown
    #[cfg_attr(test, specta(type = Option<specta_typescript::Unknown>))]
    pub details: Option<Box<serde_json::Value>>,
    pub i18n_key: Option<String>,
}

impl IpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            i18n_key: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(Box::new(details));
        self
    }

    /// Skills 领域错误：`details = { context, suggestion }`，前端按 code 翻译
    pub fn skill(code: ErrorCode, context: &[(&str, &str)], suggestion: Option<&str>) -> Self {
        let mut ctx_map = serde_json::Map::new();
        for (key, value) in context {
            ctx_map.insert(key.to_string(), serde_json::Value::from(*value));
        }
        let message = if context.is_empty() {
            code.as_str().to_string()
        } else {
            let pairs: Vec<String> = context.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("{} ({})", code.as_str(), pairs.join(", "))
        };
        Self::new(code, message).with_details(serde_json::json!({
            "context": ctx_map,
            "suggestion": suggestion,
        }))
    }

    fn from_app_error_ref(err: &AppError, message: String) -> Self {
        use serde_json::json;

        let (code, details, i18n_key) = match err {
            AppError::Config(_) => (ErrorCode::Config, None, None),
            AppError::InvalidInput(_) => (ErrorCode::InvalidInput, None, None),
            AppError::Io { path, .. } => (ErrorCode::Io, Some(json!({ "path": path })), None),
            AppError::IoContext { .. } => (ErrorCode::Io, None, None),
            AppError::Json { path, .. } => (ErrorCode::Json, Some(json!({ "path": path })), None),
            AppError::JsonSerialize { .. } => (ErrorCode::Json, None, None),
            AppError::Toml { path, .. } => (ErrorCode::Toml, Some(json!({ "path": path })), None),
            AppError::Lock(_) => (ErrorCode::Lock, None, None),
            AppError::McpValidation(_) => (ErrorCode::McpValidation, None, None),
            AppError::Message(_) => (ErrorCode::Unknown, None, None),
            AppError::HttpStatus { status, .. } => (
                ErrorCode::HttpStatus,
                Some(json!({ "status": status })),
                None,
            ),
            AppError::Localized { key, zh, en } => (
                ErrorCode::Localized,
                Some(json!({ "zh": zh, "en": en })),
                Some((*key).to_string()),
            ),
            AppError::Database(_) => (ErrorCode::Database, None, None),
            AppError::OmoConfigNotFound => (ErrorCode::OmoConfigNotFound, None, None),
            AppError::AllProvidersCircuitOpen => (ErrorCode::CircuitOpen, None, None),
            AppError::NoProvidersConfigured => (ErrorCode::NoProvidersConfigured, None, None),
            AppError::Unsupported { app, feature } => (
                ErrorCode::Unsupported,
                Some(json!({ "app": app, "feature": feature })),
                None,
            ),
        };
        Self {
            code,
            message,
            details: details.map(Box::new),
            i18n_key,
        }
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for IpcError {}

impl From<AppError> for IpcError {
    fn from(err: AppError) -> Self {
        let message = err.to_string();
        Self::from_app_error_ref(&err, message)
    }
}

impl From<anyhow::Error> for IpcError {
    fn from(err: anyhow::Error) -> Self {
        if let Some(ipc) = err.downcast_ref::<IpcError>() {
            return ipc.clone();
        }
        let message = err.to_string();
        if let Some(app) = err.downcast_ref::<AppError>() {
            return Self::from_app_error_ref(app, message);
        }
        Self::new(ErrorCode::Unknown, message)
    }
}

impl From<rusqlite::Error> for IpcError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::from(err).into()
    }
}

/// 过渡期：尚未结构化的 String 错误统一映射为 `UNKNOWN`
impl From<String> for IpcError {
    fn from(message: String) -> Self {
        Self::new(ErrorCode::Unknown, message)
    }
}

impl From<&str> for IpcError {
    fn from(message: &str) -> Self {
        Self::new(ErrorCode::Unknown, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 强制 `ALL` 与枚举保持同步：新增变体后此 match 无法编译
    fn variant_index(code: ErrorCode) -> usize {
        match code {
            ErrorCode::Unknown => 0,
            ErrorCode::Config => 1,
            ErrorCode::InvalidInput => 2,
            ErrorCode::Io => 3,
            ErrorCode::Json => 4,
            ErrorCode::Toml => 5,
            ErrorCode::Lock => 6,
            ErrorCode::McpValidation => 7,
            ErrorCode::HttpStatus => 8,
            ErrorCode::Localized => 9,
            ErrorCode::Database => 10,
            ErrorCode::OmoConfigNotFound => 11,
            ErrorCode::CircuitOpen => 12,
            ErrorCode::NoProvidersConfigured => 13,
            ErrorCode::Unsupported => 14,
            ErrorCode::SkillNotFound => 15,
            ErrorCode::InvalidSkillDirectory => 16,
            ErrorCode::SkillDirectoryConflict => 17,
            ErrorCode::SkillDirNotFound => 18,
            ErrorCode::DownloadTimeout => 19,
            ErrorCode::DownloadFailed => 20,
            ErrorCode::EmptyArchive => 21,
            ErrorCode::NoSkillsInZip => 22,
            ErrorCode::ToolNotInstalled => 23,
            ErrorCode::TakeoverActive => 24,
            ErrorCode::DbVersionTooNew => 25,
        }
    }

    #[test]
    fn error_code_all_lists_every_variant_once() {
        for (i, code) in ErrorCode::ALL.iter().enumerate() {
            assert_eq!(variant_index(*code), i, "{code:?} out of order in ALL");
        }
        assert_eq!(ErrorCode::ALL.len(), 26);
    }

    #[test]
    fn error_code_serializes_to_stable_string_for_every_variant() {
        let expected = [
            "UNKNOWN",
            "CONFIG",
            "INVALID_INPUT",
            "IO",
            "JSON",
            "TOML",
            "LOCK",
            "MCP_VALIDATION",
            "HTTP_STATUS",
            "LOCALIZED",
            "DATABASE",
            "OMO_CONFIG_NOT_FOUND",
            "CIRCUIT_OPEN",
            "NO_PROVIDERS_CONFIGURED",
            "UNSUPPORTED",
            "SKILL_NOT_FOUND",
            "INVALID_SKILL_DIRECTORY",
            "SKILL_DIRECTORY_CONFLICT",
            "SKILL_DIR_NOT_FOUND",
            "DOWNLOAD_TIMEOUT",
            "DOWNLOAD_FAILED",
            "EMPTY_ARCHIVE",
            "NO_SKILLS_IN_ZIP",
            "TOOL_NOT_INSTALLED",
            "TAKEOVER_ACTIVE",
            "DB_VERSION_TOO_NEW",
        ];
        assert_eq!(ErrorCode::ALL.len(), expected.len());
        for (code, wire) in ErrorCode::ALL.iter().zip(expected) {
            assert_eq!(serde_json::to_value(code).unwrap(), json!(wire));
            assert_eq!(code.as_str(), wire);
            assert_eq!(code.to_string(), wire);
            let back: ErrorCode = serde_json::from_value(json!(wire)).unwrap();
            assert_eq!(back, *code);
        }
    }

    #[test]
    fn ipc_error_serializes_with_camel_case_fields() {
        let err = IpcError::new(ErrorCode::Config, "bad");
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            json!({ "code": "CONFIG", "message": "bad", "details": null, "i18nKey": null })
        );
    }

    #[test]
    fn ipc_error_from_localized_app_error_keeps_key() {
        let err: IpcError = AppError::localized("provider.notFound", "未找到", "not found").into();
        assert_eq!(err.code, ErrorCode::Localized);
        assert_eq!(err.i18n_key.as_deref(), Some("provider.notFound"));
        assert_eq!(err.message, "未找到 (not found)");
        assert_eq!(
            err.details.as_deref(),
            Some(&json!({ "zh": "未找到", "en": "not found" }))
        );
    }

    #[test]
    fn ipc_error_from_app_error_maps_codes_and_keeps_message() {
        let cases: Vec<(AppError, ErrorCode)> = vec![
            (AppError::Config("x".into()), ErrorCode::Config),
            (AppError::InvalidInput("x".into()), ErrorCode::InvalidInput),
            (
                AppError::io("/tmp/a", std::io::Error::other("boom")),
                ErrorCode::Io,
            ),
            (AppError::Lock("x".into()), ErrorCode::Lock),
            (
                AppError::McpValidation("x".into()),
                ErrorCode::McpValidation,
            ),
            (AppError::Message("x".into()), ErrorCode::Unknown),
            (AppError::Database("x".into()), ErrorCode::Database),
            (AppError::OmoConfigNotFound, ErrorCode::OmoConfigNotFound),
            (AppError::AllProvidersCircuitOpen, ErrorCode::CircuitOpen),
            (
                AppError::NoProvidersConfigured,
                ErrorCode::NoProvidersConfigured,
            ),
        ];
        for (app, code) in cases {
            let message = app.to_string();
            let err = IpcError::from(app);
            assert_eq!(err.code, code);
            assert_eq!(err.message, message);
        }

        let err = IpcError::from(AppError::HttpStatus {
            status: 503,
            body: "down".into(),
        });
        assert_eq!(err.code, ErrorCode::HttpStatus);
        assert_eq!(err.details.as_deref(), Some(&json!({ "status": 503 })));
    }

    #[test]
    fn ipc_error_from_string_is_unknown() {
        let err: IpcError = String::from("legacy failure").into();
        assert_eq!(err.code, ErrorCode::Unknown);
        assert_eq!(err.message, "legacy failure");
        assert_eq!(err.i18n_key, None);
    }

    #[test]
    fn ipc_error_from_anyhow_recovers_wrapped_errors() {
        let skill = IpcError::skill(
            ErrorCode::DownloadTimeout,
            &[("owner", "o"), ("name", "n")],
            Some("checkNetwork"),
        );
        let recovered = IpcError::from(anyhow::anyhow!(skill.clone()));
        assert_eq!(recovered, skill);
        assert_eq!(
            recovered.details.as_deref(),
            Some(&json!({
                "context": { "owner": "o", "name": "n" },
                "suggestion": "checkNetwork",
            }))
        );

        let app = IpcError::from(anyhow::Error::new(AppError::AllProvidersCircuitOpen));
        assert_eq!(app.code, ErrorCode::CircuitOpen);

        let plain = IpcError::from(anyhow::anyhow!("plain"));
        assert_eq!(plain.code, ErrorCode::Unknown);
        assert_eq!(plain.message, "plain");
    }
}
