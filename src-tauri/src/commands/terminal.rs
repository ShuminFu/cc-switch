//! provider 专属终端：按 provider 配置生成临时 claude 配置并在首选终端中启动。
//! 终端选择、临时文件与引号处理见 `platform::terminal`。

use crate::app_config::AppType;
use crate::error::IpcError;
use crate::platform::terminal::{launch_terminal_with_env, resolve_launch_cwd};
use crate::services::ProviderService;
use std::str::FromStr;
use tauri::State;

/// 打开指定提供商的终端
///
/// 根据提供商配置的环境变量启动一个带有该提供商特定设置的终端
/// 无需检查是否为当前激活的提供商，任何提供商都可以打开终端
#[tauri::command]
pub async fn open_provider_terminal(
    state: State<'_, crate::store::AppState>,
    app: String,
    provider_id: String,
    cwd: Option<String>,
) -> Result<bool, IpcError> {
    let app_type = AppType::from_str(&app).map_err(IpcError::from)?;
    let launch_cwd = resolve_launch_cwd(cwd)?;

    // 获取提供商配置
    let providers = ProviderService::list(state.inner(), app_type.clone())
        .map_err(|e| format!("获取提供商列表失败: {e}"))?;

    let provider = providers
        .get(&provider_id)
        .ok_or_else(|| format!("提供商 {provider_id} 不存在"))?;

    // 从提供商配置中提取环境变量
    let config = &provider.settings_config;
    let env_vars = extract_env_vars_from_config(config, &app_type);

    // 根据平台启动终端，传入提供商ID用于生成唯一的配置文件名
    launch_terminal_with_env(env_vars, &provider_id, launch_cwd.as_deref())
        .map_err(|e| format!("启动终端失败: {e}"))?;

    Ok(true)
}

/// 从提供商配置中提取环境变量
fn extract_env_vars_from_config(
    config: &serde_json::Value,
    app_type: &AppType,
) -> Vec<(String, String)> {
    let mut env_vars = Vec::new();

    let Some(obj) = config.as_object() else {
        return env_vars;
    };

    // 处理 env 字段（Claude/Gemini 通用）
    if let Some(env) = obj.get("env").and_then(|v| v.as_object()) {
        for (key, value) in env {
            if let Some(str_val) = value.as_str() {
                env_vars.push((key.clone(), str_val.to_string()));
            }
        }

        // 处理 base_url: 根据应用类型添加对应的环境变量
        if let Some(key) = crate::apps::adapter(app_type).env_base_url_key() {
            if let Some(url_str) = env.get(key).and_then(|v| v.as_str()) {
                env_vars.push((key.to_string(), url_str.to_string()));
            }
        }
    }

    // Codex 使用 auth 字段转换为 OPENAI_API_KEY
    if *app_type == AppType::Codex {
        if let Some(auth) = obj.get("auth").and_then(|v| v.as_str()) {
            env_vars.push(("OPENAI_API_KEY".to_string(), auth.to_string()));
        }
    }

    // Gemini 使用 api_key 字段转换为 GEMINI_API_KEY
    if *app_type == AppType::Gemini {
        if let Some(api_key) = obj.get("api_key").and_then(|v| v.as_str()) {
            env_vars.push(("GEMINI_API_KEY".to_string(), api_key.to_string()));
        }
    }

    env_vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_url_entries(app_type: &AppType) -> Vec<String> {
        let config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://anthropic.example",
                "GOOGLE_GEMINI_BASE_URL": "https://gemini.example"
            }
        });
        extract_env_vars_from_config(&config, app_type)
            .into_iter()
            .map(|(key, _)| key)
            .collect()
    }

    /// env 原样透传后，按应用适配器声明的 base_url 键再追加一次（与 CCS-236 前的 match 一致）
    #[test]
    fn base_url_key_follows_app_adapter() {
        let all_env = vec![
            "ANTHROPIC_BASE_URL".to_string(),
            "GOOGLE_GEMINI_BASE_URL".to_string(),
        ];
        for app_type in AppType::all() {
            let mut expected = all_env.clone();
            match app_type {
                AppType::Claude | AppType::ClaudeDesktop => {
                    expected.push("ANTHROPIC_BASE_URL".to_string())
                }
                AppType::Gemini => expected.push("GOOGLE_GEMINI_BASE_URL".to_string()),
                _ => {}
            }
            assert_eq!(base_url_entries(&app_type), expected, "{app_type:?}");
        }
    }

    #[test]
    fn codex_and_gemini_credentials_become_api_key_env() {
        let codex = extract_env_vars_from_config(&json!({ "auth": "sk-codex" }), &AppType::Codex);
        assert_eq!(
            codex,
            vec![("OPENAI_API_KEY".to_string(), "sk-codex".to_string())]
        );

        let gemini = extract_env_vars_from_config(&json!({ "api_key": "g-key" }), &AppType::Gemini);
        assert_eq!(
            gemini,
            vec![("GEMINI_API_KEY".to_string(), "g-key".to_string())]
        );
    }
}
