use std::path::PathBuf;

use crate::app_config::AppType;
use crate::apps::AppCaps;
use crate::codex_config::get_codex_auth_path;
use crate::config::get_claude_settings_path;
use crate::error::AppError;
use crate::openclaw_config::get_openclaw_dir;
use crate::opencode_config::get_opencode_dir;

/// 返回指定应用所使用的提示词文件路径。
///
/// 不具备 [`AppCaps::PROMPTS`] 能力的应用（如 Claude Desktop）返回
/// [`AppError::Unsupported`]，而不是 panic。
pub fn prompt_file_path(app: &AppType) -> Result<PathBuf, AppError> {
    if !app.descriptor().has(AppCaps::PROMPTS) {
        return Err(AppError::unsupported(app, "Prompts"));
    }

    if let Some(adapter) = crate::apps::adapter(app) {
        return Ok(adapter.prompt_file());
    }

    let (base_dir, filename): (PathBuf, &str) = match app {
        AppType::Claude => (
            get_base_dir_with_fallback(get_claude_settings_path(), ".claude")?,
            "CLAUDE.md",
        ),
        AppType::Codex => (
            get_base_dir_with_fallback(get_codex_auth_path(), ".codex")?,
            "AGENTS.md",
        ),
        AppType::OpenCode => (get_opencode_dir(), "AGENTS.md"),
        AppType::OpenClaw => (get_openclaw_dir(), "AGENTS.md"),
        AppType::Hermes => (crate::hermes_config::get_hermes_dir(), "AGENTS.md"),
        // Claude Desktop 无提示词文件；已迁移到 AppAdapter 的应用在上方分派
        _ => return Err(AppError::unsupported(app, "Prompts")),
    };

    Ok(base_dir.join(filename))
}

fn get_base_dir_with_fallback(
    primary_path: PathBuf,
    fallback_dir: &str,
) -> Result<PathBuf, AppError> {
    primary_path
        .parent()
        .map(|p| p.to_path_buf())
        .or_else(|| dirs::home_dir().map(|h| h.join(fallback_dir)))
        .ok_or_else(|| {
            AppError::localized(
                "home_dir_not_found",
                format!("无法确定 {fallback_dir} 配置目录：用户主目录不存在"),
                format!("Cannot determine {fallback_dir} config directory: user home not found"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::APPS;

    /// 每个应用的提示词路径要么是 Ok，要么是 Unsupported，绝不 panic。
    #[test]
    fn prompt_path_is_ok_or_unsupported_for_every_app() {
        for d in APPS {
            let result = std::panic::catch_unwind(|| prompt_file_path(&d.app));
            let result =
                result.unwrap_or_else(|_| panic!("prompt_file_path panicked for {}", d.slug));
            match result {
                Ok(path) => {
                    assert!(d.has(AppCaps::PROMPTS), "{} has no PROMPTS cap", d.slug);
                    assert!(path.extension().is_some_and(|e| e == "md"), "{path:?}");
                }
                Err(AppError::Unsupported { app, feature }) => {
                    assert!(
                        !d.has(AppCaps::PROMPTS),
                        "{} should support prompts",
                        d.slug
                    );
                    assert_eq!(app, d.display_name);
                    assert_eq!(feature, "Prompts");
                }
                Err(other) => panic!("unexpected error for {}: {other}", d.slug),
            }
        }
    }

    #[test]
    fn claude_desktop_prompt_path_is_unsupported() {
        let err = prompt_file_path(&AppType::ClaudeDesktop).unwrap_err();
        assert!(matches!(err, AppError::Unsupported { .. }), "{err:?}");
        assert!(err.to_string().contains("Claude Desktop"));
    }
}
