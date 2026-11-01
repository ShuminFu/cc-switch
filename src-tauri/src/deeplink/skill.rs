//! Skill import from deep link
//!
//! Handles importing skill repository configurations via ccswitch:// URLs.

use super::DeepLinkImportRequest;
use crate::error::AppError;
use crate::services::skill::SkillRepo;
use crate::store::AppState;

/// Import a skill from deep link request
pub fn import_skill_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    // Verify this is a skill request
    if request.resource != "skill" {
        return Err(AppError::InvalidInput(format!(
            "Expected skill resource, got '{}'",
            request.resource
        )));
    }

    // Parse repo
    let repo_str = request
        .repo
        .ok_or_else(|| AppError::InvalidInput("Missing 'repo' field for skill".to_string()))?;

    // The request reaches this function from the webview after parsing, so
    // re-validate here instead of trusting the parser ran (SEC-24): owner,
    // name and branch end up in a github.com archive URL and in the DB.
    let parts: Vec<&str> = repo_str.split('/').collect();
    if parts.len() != 2
        || !parts
            .iter()
            .all(|part| super::parser::is_valid_repo_part(part))
    {
        return Err(AppError::InvalidInput(format!(
            "Invalid repo format: expected 'owner/name', got '{repo_str}'"
        )));
    }
    let owner = parts[0].to_string();
    let name = parts[1].to_string();
    let branch = request.branch.unwrap_or_else(|| "main".to_string());
    if !super::parser::is_valid_branch(&branch) {
        return Err(AppError::InvalidInput(format!(
            "Invalid branch name: '{branch}'"
        )));
    }

    // Create SkillRepo
    let repo = SkillRepo {
        owner: owner.clone(),
        name: name.clone(),
        branch,
        enabled: request.enabled.unwrap_or(true),
    };

    // Save using Database
    state.db.save_skill_repo(&repo)?;

    log::info!("Successfully added skill repo '{owner}/{name}'");

    Ok(format!("{owner}/{name}"))
}
