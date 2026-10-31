//! Typed errors of the proxy/takeover service (SVC-22).
//!
//! The service used to return `Result<_, String>`; every failure was a
//! formatted Chinese sentence and the kind of failure was lost. A
//! [`TakeoverError`] keeps that exact sentence as its `Display` text (the
//! proxy commands still return it verbatim, so the UI shows the same message)
//! and adds a stable [`TakeoverErrorKind`], exposed as a `takeover.*` code when
//! the error is converted into an [`AppError`].

use crate::error::AppError;

/// What failed. The `code()` is a stable i18n key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeoverErrorKind {
    /// Reading or writing the proxy configuration rows.
    Config,
    /// The proxy server could not be started.
    StartFailed,
    /// The proxy server could not be stopped.
    StopFailed,
    /// An operation needs a running proxy server.
    NotRunning,
    /// The requested listen address/port cannot be bound.
    PortUnavailable,
    /// Backing up a live config before takeover.
    Backup,
    /// Reading a live client config file.
    LiveRead,
    /// Writing a live client config file.
    LiveWrite,
    /// Restoring live configs from the takeover backup.
    Restore,
    /// Provider lookup or provider/live synchronisation.
    Provider,
    /// The app does not support proxy takeover.
    UnsupportedApp,
    /// Anything not classified above.
    Other,
}

impl TakeoverErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Config => "takeover.config",
            Self::StartFailed => "takeover.start_failed",
            Self::StopFailed => "takeover.stop_failed",
            Self::NotRunning => "takeover.not_running",
            Self::PortUnavailable => "takeover.port_unavailable",
            Self::Backup => "takeover.backup",
            Self::LiveRead => "takeover.live_read",
            Self::LiveWrite => "takeover.live_write",
            Self::Restore => "takeover.restore",
            Self::Provider => "takeover.provider",
            Self::UnsupportedApp => "takeover.unsupported_app",
            Self::Other => "takeover.failed",
        }
    }
}

/// A proxy/takeover failure: a kind plus the user-facing message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeoverError {
    kind: TakeoverErrorKind,
    message: String,
}

impl TakeoverError {
    pub fn new(kind: TakeoverErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> TakeoverErrorKind {
        self.kind
    }

    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// Keep the kind, prefix the message (`"{context}: {message}"`).
    pub fn context(self, context: impl std::fmt::Display) -> Self {
        Self {
            kind: self.kind,
            message: format!("{context}: {}", self.message),
        }
    }

    /// `map_err` helper: `.map_err(TakeoverError::with(Kind::Config, "获取代理配置失败"))`
    /// yields `"获取代理配置失败: {e}"`, the text the service used to format.
    pub fn with<E: std::fmt::Display>(
        kind: TakeoverErrorKind,
        context: &'static str,
    ) -> impl Fn(E) -> Self {
        move |e| Self::new(kind, format!("{context}: {e}"))
    }
}

impl std::fmt::Display for TakeoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TakeoverError {}

/// Unclassified messages (helpers that still build plain strings).
impl From<String> for TakeoverError {
    fn from(message: String) -> Self {
        Self::new(TakeoverErrorKind::Other, message)
    }
}

impl From<&str> for TakeoverError {
    fn from(message: &str) -> Self {
        Self::new(TakeoverErrorKind::Other, message)
    }
}

/// Lower-level `AppError`s keep their text; the kind is inferred from the
/// variant where it is meaningful.
impl From<AppError> for TakeoverError {
    fn from(err: AppError) -> Self {
        let kind = match &err {
            AppError::Database(_) => TakeoverErrorKind::Config,
            AppError::Io { .. } | AppError::IoContext { .. } => TakeoverErrorKind::LiveWrite,
            AppError::Json { .. } | AppError::Toml { .. } => TakeoverErrorKind::LiveRead,
            AppError::Localized {
                key: "proxy.port_unavailable",
                ..
            } => TakeoverErrorKind::PortUnavailable,
            _ => TakeoverErrorKind::Other,
        };
        Self::new(kind, err.to_string())
    }
}

/// The proxy commands still return `Result<_, String>`: the text is unchanged.
impl From<TakeoverError> for String {
    fn from(err: TakeoverError) -> Self {
        err.message
    }
}

/// For `AppError`-returning callers: a `takeover.*` code with the message
/// (the service's messages are Chinese, so `zh` and `en` both carry it until
/// the frontend translates the code).
impl From<TakeoverError> for AppError {
    fn from(err: TakeoverError) -> Self {
        AppError::Localized {
            key: err.code(),
            zh: err.message.clone(),
            en: err.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_string_keep_the_service_message() {
        let err = TakeoverError::new(TakeoverErrorKind::NotRunning, "代理服务器未运行");
        assert_eq!(err.to_string(), "代理服务器未运行");
        assert_eq!(String::from(err.clone()), "代理服务器未运行");
        let app: AppError = err.into();
        assert_eq!(app.code(), "takeover.not_running");
        let payload = app.to_payload();
        assert_eq!(payload.code, "takeover.not_running");
    }

    #[test]
    fn with_formats_like_the_old_map_err() {
        let make = TakeoverError::with::<&str>(TakeoverErrorKind::Config, "获取代理配置失败");
        let err = make("db locked");
        assert_eq!(err.to_string(), "获取代理配置失败: db locked");
        assert_eq!(err.code(), "takeover.config");
        assert_eq!(
            err.context("启动失败").to_string(),
            "启动失败: 获取代理配置失败: db locked"
        );
    }

    #[test]
    fn app_errors_map_to_a_kind() {
        let err: TakeoverError = AppError::Database("x".into()).into();
        assert_eq!(err.kind(), TakeoverErrorKind::Config);
        let err: TakeoverError = AppError::localized("proxy.port_unavailable", "z", "e").into();
        assert_eq!(err.kind(), TakeoverErrorKind::PortUnavailable);
        let err: TakeoverError = String::from("plain").into();
        assert_eq!(err.code(), "takeover.failed");
    }
}
