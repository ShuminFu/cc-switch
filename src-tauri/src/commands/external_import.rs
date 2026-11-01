//! Import from adjacent tools' configs (PRODUCT-26).
//!
//! `scan_external_imports` is a dry run: it returns what each detected source
//! would create, with API keys masked. `import_external_config` re-reads the
//! source from disk (the webview never sends keys back) and adds the selected
//! providers, optionally with the source's routing rules.
#![allow(non_snake_case)]

use std::str::FromStr;

use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;

use crate::adapters::credential_fingerprint;
use crate::adapters::external_import::{self, ExternalImportPreview};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta};
use crate::proxy::routing;
use crate::services::ProviderService;
use crate::store::AppState;

use super::{timed_blocking, timed_blocking_with_state};

/// Outcome of `import_external_config`.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalImportResult {
    /// Provider ids created.
    pub imported: Vec<String>,
    /// Provider ids skipped because the same endpoint+key (or id) exists.
    pub skipped: Vec<String>,
    /// Routing rules appended to the target app's routing config.
    pub rules_imported: usize,
}

fn mask_secret(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        return "*".repeat(chars.len());
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

/// The preview with every `*_TOKEN` / `*_KEY` env value masked.
pub fn masked(mut preview: ExternalImportPreview) -> ExternalImportPreview {
    for provider in &mut preview.providers {
        if let Some(env) = provider
            .settings_config
            .get_mut("env")
            .and_then(Value::as_object_mut)
        {
            for (name, value) in env.iter_mut() {
                if name.ends_with("_TOKEN") || name.ends_with("_KEY") {
                    if let Some(s) = value.as_str() {
                        *value = Value::String(mask_secret(s));
                    }
                }
            }
        }
    }
    preview
}

/// Detected configs of other tools and what importing them would create
/// (dry run, keys masked).
#[tauri::command]
pub async fn scan_external_imports() -> Result<Vec<ExternalImportPreview>, AppError> {
    timed_blocking("scan_external_imports", || {
        Ok::<_, AppError>(
            external_import::scan()
                .into_iter()
                .map(masked)
                .collect::<Vec<_>>(),
        )
    })
    .await
}

fn existing_fingerprints(state: &AppState, app: &AppType) -> Result<Vec<String>, AppError> {
    let providers = ProviderService::list(state, app.clone())?;
    Ok(providers
        .values()
        .filter_map(|p| {
            let env = p.settings_config.get("env")?;
            let url = env.get("ANTHROPIC_BASE_URL")?.as_str()?;
            let key = env
                .get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| env.get("ANTHROPIC_API_KEY"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            Some(credential_fingerprint(url.trim_end_matches('/'), key))
        })
        .collect())
}

/// Import `preview` into `state` (shared by the command and tests).
pub fn import_preview(
    state: &AppState,
    preview: &ExternalImportPreview,
    provider_ids: Option<&[String]>,
    import_routing_rules: bool,
) -> Result<ExternalImportResult, AppError> {
    let mut result = ExternalImportResult::default();
    let mut available: Vec<String> = Vec::new();
    for candidate in &preview.providers {
        if provider_ids.is_some_and(|ids| !ids.contains(&candidate.id_hint)) {
            continue;
        }
        let app = AppType::from_str(&candidate.app)?;
        let existing = ProviderService::list(state, app.clone())?;
        if existing.contains_key(&candidate.id_hint)
            || existing_fingerprints(state, &app)?.contains(&candidate.fingerprint)
        {
            result.skipped.push(candidate.id_hint.clone());
            available.push(candidate.id_hint.clone());
            continue;
        }
        let mut provider = Provider::with_id(
            candidate.id_hint.clone(),
            candidate.name.clone(),
            candidate.settings_config.clone(),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some(candidate.api_format.clone()),
            ..Default::default()
        });
        ProviderService::add(state, app, provider, false)?;
        result.imported.push(candidate.id_hint.clone());
        available.push(candidate.id_hint.clone());
    }

    if import_routing_rules && !preview.routing_rules.is_empty() {
        let app = preview
            .providers
            .first()
            .map(|p| p.app.clone())
            .unwrap_or_else(|| "claude".to_string());
        let mut config = routing::load_routing_config(&state.db, &app)?;
        for rule in &preview.routing_rules {
            if !rule.providers.iter().all(|id| available.contains(id)) {
                continue;
            }
            if config
                .model_rules
                .iter()
                .any(|r| r.pattern == rule.pattern && r.providers == rule.providers)
            {
                continue;
            }
            config.model_rules.push(rule.clone());
            result.rules_imported += 1;
        }
        if result.rules_imported > 0 {
            routing::save_routing_config(&state.db, &app, &config)?;
        }
    }
    Ok(result)
}

/// Import providers (all, or the `providerIds` subset) and optionally the
/// routing rules from `source` (e.g. `claude-code-router`).
#[tauri::command]
pub async fn import_external_config(
    app_handle: AppHandle,
    source: String,
    providerIds: Option<Vec<String>>,
    importRoutingRules: Option<bool>,
) -> Result<ExternalImportResult, AppError> {
    timed_blocking_with_state("import_external_config", app_handle, move |state| {
        let preview = external_import::preview_for(&source)?;
        import_preview(
            state,
            &preview,
            providerIds.as_deref(),
            importRoutingRules.unwrap_or(true),
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;
    use std::sync::Arc;

    fn preview() -> ExternalImportPreview {
        external_import::preview_claude_code_router(
            &json!({
                "Providers": [
                    {"name": "a", "api_base_url": "https://a.example/v1/chat/completions", "api_key": "sk-aaaa-1111", "models": ["m-a"]},
                    {"name": "b", "api_base_url": "https://b.example/v1/chat/completions", "api_key": "sk-bbbb-2222", "models": ["m-b"]},
                    {"name": "c", "api_base_url": "https://c.example/v1/chat/completions", "api_key": "sk-cccc-3333", "models": ["m-c"]},
                    {"name": "d", "api_base_url": "https://d.example/v1/chat/completions", "api_key": "sk-dddd-4444", "models": ["m-d"]}
                ],
                "Router": {"default": "a,m-a", "background": "b,m-b", "think": "c,m-c"}
            }),
            Path::new("config.json"),
        )
    }

    #[test]
    fn masked_preview_hides_keys() {
        let p = masked(preview());
        let token = p.providers[0].settings_config["env"]["ANTHROPIC_AUTH_TOKEN"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(token, "sk-a…1111");
        assert_eq!(
            p.providers[0].settings_config["env"]["ANTHROPIC_BASE_URL"],
            "https://a.example/v1"
        );
    }

    #[test]
    fn imports_providers_with_keys_and_rules_once() {
        let _home = crate::test_support::isolated_home();
        let db = Arc::new(crate::database::Database::memory().unwrap());
        let state = AppState::new(db.clone());
        let result = import_preview(&state, &preview(), None, true).unwrap();
        assert_eq!(result.imported.len(), 4);
        assert_eq!(result.rules_imported, 2);

        let providers = ProviderService::list(&state, AppType::Claude).unwrap();
        let a = &providers["ccr-a"];
        assert_eq!(
            a.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
            "sk-aaaa-1111"
        );
        assert_eq!(
            a.meta.as_ref().unwrap().api_format.as_deref(),
            Some("openai_chat")
        );
        let routing = routing::load_routing_config(&db, "claude").unwrap();
        assert_eq!(routing.model_rules.len(), 2);

        // Importing again skips everything.
        let again = import_preview(&state, &preview(), None, true).unwrap();
        assert!(again.imported.is_empty());
        assert_eq!(again.skipped.len(), 4);
        assert_eq!(again.rules_imported, 0);
    }
}
