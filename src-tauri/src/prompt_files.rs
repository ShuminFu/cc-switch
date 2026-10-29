use std::path::PathBuf;

use crate::app_config::AppType;
use crate::apps::AppCaps;
use crate::error::AppError;

/// 返回指定应用所使用的提示词文件路径。
///
/// 不具备 [`AppCaps::PROMPTS`] 能力的应用（如 Claude Desktop）返回
/// [`AppError::Unsupported`]，而不是 panic。
pub fn prompt_file_path(app: &AppType) -> Result<PathBuf, AppError> {
    if !app.descriptor().has(AppCaps::PROMPTS) {
        return Err(AppError::unsupported(app, "Prompts"));
    }

    crate::apps::adapter(app)
        .prompt_file()
        .ok_or_else(|| AppError::unsupported(app, "Prompts"))
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
