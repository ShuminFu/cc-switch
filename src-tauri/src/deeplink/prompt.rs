//! Prompt import from deep link
//!
//! Handles importing prompt configurations via ccswitch:// URLs.

use super::utils::decode_base64_param;
use super::DeepLinkImportRequest;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::services::PromptService;
use crate::store::AppState;
use crate::AppType;
use std::str::FromStr;

/// Import a prompt from deep link request
pub fn import_prompt_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    // Verify this is a prompt request
    if request.resource != "prompt" {
        return Err(AppError::InvalidInput(format!(
            "Expected prompt resource, got '{}'",
            request.resource
        )));
    }

    // Extract required fields
    let app_str = request
        .app
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' field for prompt".to_string()))?;

    let name = request
        .name
        .ok_or_else(|| AppError::InvalidInput("Missing 'name' field for prompt".to_string()))?;

    // Parse app type
    let app_type = AppType::from_str(app_str)
        .map_err(|_| AppError::InvalidInput(format!("Invalid app type: {app_str}")))?;

    // Decode content
    let content_b64 = request
        .content
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("Missing 'content' field for prompt".to_string()))?;

    let content = decode_base64_param("content", content_b64)?;
    let content = String::from_utf8(content)
        .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in content: {e}")))?;

    // Generate ID
    let timestamp = chrono::Utc::now().timestamp_millis();
    let sanitized_name = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase();
    if sanitized_name.is_empty() {
        return Err(AppError::InvalidInput(
            "Prompt name must contain at least one letter or digit".to_string(),
        ));
    }
    let id = format!("{sanitized_name}-{timestamp}");

    // Only an explicit opt-in (set by the confirmation dialog) enables it.
    let should_enable = request.enabled == Some(true);

    // Create Prompt (initially disabled)
    let prompt = Prompt {
        id: id.clone(),
        name: name.clone(),
        content,
        description: request.description,
        enabled: false, // Always start as disabled, will be enabled later if needed
        created_at: Some(timestamp),
        updated_at: Some(timestamp),
    };

    // Save using PromptService
    PromptService::upsert_prompt(state, app_type.clone(), &id, prompt)?;

    // If enabled flag is set, enable this prompt (which will disable others).
    // The live file is always snapshotted first: enable_prompt would otherwise
    // fold it into the previously enabled prompt, silently rewriting it (SESS-07).
    if should_enable {
        backup_live_prompt(state, &app_type, &name, timestamp)?;
        PromptService::enable_prompt(state, app_type, &id)?;
        log::info!("Successfully imported and enabled prompt '{name}' for {app_str}");
    } else {
        log::info!("Successfully imported prompt '{name}' for {app_str} (disabled)");
    }

    Ok(id)
}

/// Save the current live prompt file as a disabled backup prompt.
fn backup_live_prompt(
    state: &AppState,
    app_type: &AppType,
    imported_name: &str,
    timestamp: i64,
) -> Result<(), AppError> {
    let path = crate::prompt_files::prompt_file_path(app_type)?;
    let Ok(live) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    if live.trim().is_empty() {
        return Ok(());
    }
    let id = format!("deeplink-backup-{timestamp}");
    let backup = Prompt {
        id: id.clone(),
        name: format!("Backup before importing {imported_name}"),
        content: live,
        description: Some(format!(
            "Live {} saved before a deep-link prompt was enabled",
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        )),
        enabled: false,
        created_at: Some(timestamp),
        updated_at: Some(timestamp),
    };
    PromptService::upsert_prompt(state, app_type.clone(), &id, backup)?;
    log::info!("Backed up live prompt before deep-link import: {id}");
    Ok(())
}
