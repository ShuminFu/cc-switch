//! One prompt across several apps (PRODUCT-59, first increment).
//!
//! Prompts are stored per app. This copies a prompt (same id, name and
//! content) into other apps' prompt lists through `PromptService`, so the
//! usual protections apply: a copy is saved disabled unless the target's
//! prompt with that id is already the enabled one (then its live file is
//! updated), and "activate" goes through `PromptService::enable_prompt`,
//! which backfills or backs up an externally edited live file before
//! replacing it. One edit can then be pushed to CLAUDE.md, AGENTS.md and
//! GEMINI.md at once.

use std::str::FromStr;

use serde::Serialize;
use tauri::AppHandle;

use super::audit_log::{self, AuditEvent};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::services::PromptService;
use crate::store::AppState;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PromptCopyResult {
    pub app: String,
    /// `created`, `updated`, `unchanged` or `error`.
    pub status: String,
    pub activated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn copy_one(
    state: &AppState,
    source: &Prompt,
    target: &AppType,
    activate: bool,
) -> Result<(String, bool), AppError> {
    crate::prompt_files::prompt_file_path(target)?;
    let existing = state
        .db
        .get_prompts(target.as_str())?
        .shift_remove(&source.id);
    let now = chrono::Utc::now().timestamp();
    let status = match &existing {
        Some(e) if e.content == source.content && e.name == source.name => "unchanged",
        Some(_) => "updated",
        None => "created",
    };
    if status != "unchanged" {
        let prompt = Prompt {
            enabled: existing.as_ref().is_some_and(|e| e.enabled),
            created_at: existing.as_ref().and_then(|e| e.created_at).or(Some(now)),
            updated_at: Some(now),
            ..source.clone()
        };
        PromptService::upsert_prompt(state, target.clone(), &source.id, prompt)?;
    }
    let already_enabled = existing.as_ref().is_some_and(|e| e.enabled);
    let activated = activate && !already_enabled;
    if activated {
        PromptService::enable_prompt(state, target.clone(), &source.id)?;
    }
    Ok((
        status.to_string(),
        activated || (activate && already_enabled),
    ))
}

fn copy_in(
    state: &AppState,
    source_app: &AppType,
    prompt_id: &str,
    targets: &[AppType],
    activate: bool,
) -> Result<Vec<PromptCopyResult>, AppError> {
    let source = state
        .db
        .get_prompts(source_app.as_str())?
        .shift_remove(prompt_id)
        .ok_or_else(|| AppError::Message(format!("Prompt {prompt_id} not found")))?;
    let mut out = Vec::new();
    for target in targets.iter().filter(|t| *t != source_app) {
        let result = match copy_one(state, &source, target, activate) {
            Ok((status, activated)) => {
                if status != "unchanged" || activated {
                    audit_log::record(
                        AuditEvent::new("ui", "prompt.copy", "prompt", prompt_id)
                            .app(target.as_str())
                            .detail(format!("from {}", source_app.as_str())),
                    );
                }
                PromptCopyResult {
                    app: target.as_str().to_string(),
                    status,
                    activated,
                    error: None,
                }
            }
            Err(e) => PromptCopyResult {
                app: target.as_str().to_string(),
                status: "error".to_string(),
                activated: false,
                error: Some(e.to_string()),
            },
        };
        out.push(result);
    }
    Ok(out)
}

/// Copy prompt `promptId` of `app` to `targetApps`; with `activate`, make it
/// the enabled prompt there (writing each app's prompt file).
#[tauri::command]
pub async fn copy_prompt_to_apps(
    handle: AppHandle,
    app: String,
    #[allow(non_snake_case)] promptId: String,
    #[allow(non_snake_case)] targetApps: Vec<String>,
    activate: bool,
) -> Result<Vec<PromptCopyResult>, String> {
    let source = AppType::from_str(&app).map_err(|e| e.to_string())?;
    let targets = targetApps
        .iter()
        .map(|a| AppType::from_str(a).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    crate::commands::run_blocking_with_state(handle, move |state| {
        copy_in(state, &source, &promptId, &targets, activate).map_err(|e| e.to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn prompt(id: &str, content: &str) -> Prompt {
        Prompt {
            id: id.to_string(),
            name: "Team rules".to_string(),
            content: content.to_string(),
            description: None,
            enabled: false,
            created_at: Some(1),
            updated_at: Some(1),
        }
    }

    #[test]
    fn one_prompt_activates_across_three_apps() {
        let home = crate::test_support::isolated_home();
        let state = AppState::new(Arc::new(crate::database::Database::memory().unwrap()));
        state
            .db
            .save_prompt("claude", &prompt("rules", "# Be concise"))
            .unwrap();
        // Codex already has an externally written AGENTS.md: enabling must
        // keep it as a backup prompt instead of losing it.
        let agents = home.path().join(".codex/AGENTS.md");
        std::fs::create_dir_all(agents.parent().unwrap()).unwrap();
        std::fs::write(&agents, "hand written").unwrap();

        let results = copy_in(
            &state,
            &AppType::Claude,
            "rules",
            &[
                AppType::Claude,
                AppType::Codex,
                AppType::Gemini,
                AppType::ClaudeDesktop,
            ],
            true,
        )
        .unwrap();
        assert_eq!(results.len(), 3, "the source app is skipped: {results:?}");
        let codex = results.iter().find(|r| r.app == "codex").unwrap();
        assert_eq!((codex.status.as_str(), codex.activated), ("created", true));
        let gemini = results.iter().find(|r| r.app == "gemini").unwrap();
        assert_eq!(
            (gemini.status.as_str(), gemini.activated),
            ("created", true)
        );
        assert_eq!(
            results
                .iter()
                .find(|r| r.app == "claude-desktop")
                .unwrap()
                .status,
            "error"
        );
        assert_eq!(std::fs::read_to_string(&agents).unwrap(), "# Be concise");
        let codex_prompts = state.db.get_prompts("codex").unwrap();
        assert!(codex_prompts["rules"].enabled);
        assert!(codex_prompts
            .values()
            .any(|p| p.id != "rules" && p.content == "hand written"));

        // A second run with the same content changes nothing.
        let again = copy_in(&state, &AppType::Claude, "rules", &[AppType::Codex], false).unwrap();
        assert_eq!(again[0].status, "unchanged");
        assert!(!again[0].activated);
    }
}
