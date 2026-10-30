//! Live configuration operations
//!
//! Handles reading and writing live configuration files. Per-app file formats live in
//! the [`crate::apps::AppAdapter`] implementations; this module only orchestrates.

use serde_json::Value;

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::mcp::McpService;
use crate::store::AppState;

pub use crate::apps::codex::update_toml_common_config_snippet;

pub(crate) fn provider_exists_in_live_config(
    app_type: &AppType,
    provider_id: &str,
) -> Result<bool, AppError> {
    crate::apps::adapter(app_type).live_has_provider(provider_id)
}

fn settings_contain_common_config(app_type: &AppType, settings: &Value, snippet: &str) -> bool {
    let trimmed = snippet.trim();
    if trimmed.is_empty() {
        return false;
    }

    crate::apps::adapter(app_type)
        .common_snippet()
        .is_some_and(|common| common.contains(settings, trimmed))
}

pub(crate) fn provider_uses_common_config(
    app_type: &AppType,
    provider: &Provider,
    snippet: Option<&str>,
) -> bool {
    match provider
        .meta
        .as_ref()
        .and_then(|meta| meta.common_config_enabled)
    {
        Some(explicit) => explicit && snippet.is_some_and(|value| !value.trim().is_empty()),
        None => snippet.is_some_and(|value| {
            settings_contain_common_config(app_type, &provider.settings_config, value)
        }),
    }
}

pub(crate) fn remove_common_config_from_settings(
    app_type: &AppType,
    settings: &Value,
    snippet: &str,
) -> Result<Value, AppError> {
    let trimmed = snippet.trim();
    if trimmed.is_empty() {
        return Ok(settings.clone());
    }

    match crate::apps::adapter(app_type).common_snippet() {
        Some(common) => common.remove(settings, trimmed),
        None => Ok(settings.clone()),
    }
}

fn apply_common_config_to_settings(
    app_type: &AppType,
    settings: &Value,
    snippet: &str,
) -> Result<Value, AppError> {
    let trimmed = snippet.trim();
    if trimmed.is_empty() {
        return Ok(settings.clone());
    }

    match crate::apps::adapter(app_type).common_snippet() {
        Some(common) => common.apply(settings, trimmed),
        None => Ok(settings.clone()),
    }
}

pub(crate) fn build_effective_settings_with_common_config(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
) -> Result<Value, AppError> {
    let snippet = db.get_config_snippet(app_type.as_str())?;
    let mut effective_settings = provider.settings_config.clone();

    if provider_uses_common_config(app_type, provider, snippet.as_deref()) {
        if let Some(snippet_text) = snippet.as_deref() {
            match apply_common_config_to_settings(app_type, &effective_settings, snippet_text) {
                Ok(settings) => effective_settings = settings,
                Err(err) => {
                    log::warn!(
                        "Failed to apply common config for {} provider '{}': {err}",
                        app_type.as_str(),
                        provider.id
                    );
                }
            }
        }
    }

    crate::apps::adapter(app_type).apply_live_defaults(provider, &mut effective_settings);

    Ok(effective_settings)
}

pub(crate) fn write_live_with_common_config(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
) -> Result<(), AppError> {
    let mut effective_provider = provider.clone();
    effective_provider.settings_config =
        build_effective_settings_with_common_config(db, app_type, provider)?;

    // 累加模式应用：被代理接管的条目写入后保留代理端点（并刷新接管备份）
    crate::services::proxy::write_entry_respecting_takeover(db, app_type, &provider.id, || {
        crate::apps::adapter(app_type).write_effective_provider(db, &effective_provider)
    })
}

/// 代理接管期间该应用的 live 是否由接管备份托管（见 [`crate::apps::AppAdapter::takeover_uses_live_backup`]）
pub(crate) fn takeover_uses_live_backup(app_type: &AppType) -> bool {
    crate::apps::adapter(app_type).takeover_uses_live_backup()
}

pub(crate) fn strip_common_config_from_live_settings(
    db: &Database,
    app_type: &AppType,
    provider: &Provider,
    live_settings: Value,
) -> Value {
    let snippet = match db.get_config_snippet(app_type.as_str()) {
        Ok(snippet) => snippet,
        Err(err) => {
            log::warn!(
                "Failed to load common config for {} while backfilling '{}': {err}",
                app_type.as_str(),
                provider.id
            );
            return restore_live_settings_for_provider_backfill(app_type, provider, live_settings);
        }
    };

    let backfill_settings = if provider_uses_common_config(app_type, provider, snippet.as_deref()) {
        match snippet.as_deref() {
            Some(snippet_text) => {
                match remove_common_config_from_settings(app_type, &live_settings, snippet_text) {
                    Ok(settings) => settings,
                    Err(err) => {
                        log::warn!(
                            "Failed to strip common config for {} provider '{}': {err}",
                            app_type.as_str(),
                            provider.id
                        );
                        live_settings
                    }
                }
            }
            None => live_settings,
        }
    } else {
        live_settings
    };

    restore_live_settings_for_provider_backfill(app_type, provider, backfill_settings)
}

fn restore_live_settings_for_provider_backfill(
    app_type: &AppType,
    provider: &Provider,
    live_settings: Value,
) -> Value {
    crate::apps::adapter(app_type).backfill_live_settings(provider, live_settings)
}

pub(crate) fn normalize_provider_common_config_for_storage(
    db: &Database,
    app_type: &AppType,
    provider: &mut Provider,
) -> Result<(), AppError> {
    let uses_common_config = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.common_config_enabled)
        .unwrap_or(false);

    if !uses_common_config {
        return Ok(());
    }

    let Some(snippet) = db.get_config_snippet(app_type.as_str())? else {
        return Ok(());
    };

    if snippet.trim().is_empty() {
        return Ok(());
    }

    match remove_common_config_from_settings(app_type, &provider.settings_config, &snippet) {
        Ok(settings) => provider.settings_config = settings,
        Err(err) => {
            log::warn!(
                "Failed to normalize common config before saving {} provider '{}': {err}",
                app_type.as_str(),
                provider.id
            );
        }
    }

    Ok(())
}

/// Sync all providers to live configuration (for additive mode apps)
///
/// Writes all providers from the database to the live configuration file.
/// Used for OpenCode and other additive mode applications.
fn sync_all_providers_to_live(state: &AppState, app_type: &AppType) -> Result<(), AppError> {
    let providers = state.db.get_all_providers(app_type.as_str())?;
    let mut synced_count = 0usize;

    for provider in providers.values() {
        if provider
            .meta
            .as_ref()
            .and_then(|meta| meta.live_config_managed)
            == Some(false)
        {
            continue;
        }

        if let Err(e) = write_live_with_common_config(state.db.as_ref(), app_type, provider) {
            log::warn!(
                "Failed to sync {:?} provider '{}' to live: {e}",
                app_type,
                provider.id
            );
            continue;
        }
        synced_count += 1;
    }

    log::info!("Synced {synced_count} {app_type:?} providers to live config");
    Ok(())
}

pub(crate) fn sync_current_provider_for_app_to_live(
    state: &AppState,
    app_type: &AppType,
) -> Result<(), AppError> {
    if app_type.is_additive_mode() {
        sync_all_providers_to_live(state, app_type)?;
    } else {
        let current_id = match crate::settings::get_effective_current_provider(&state.db, app_type)?
        {
            Some(id) => id,
            None => return Ok(()),
        };

        let providers = state.db.get_all_providers(app_type.as_str())?;
        if let Some(provider) = providers.get(&current_id) {
            write_live_with_common_config(state.db.as_ref(), app_type, provider)?;
        }
    }

    // 本函数语义是"把这个应用同步到 live"，MCP 重投影也只针对该应用；
    // 全量 sync_all_enabled 会把无关应用的 live 损坏牵连进来。投影失败
    // 上抛（不降级）：这里没有已变更的 DB 状态需要保护，调用方重试即可。
    McpService::sync_enabled_for_app(state, app_type)?;

    Ok(())
}

fn sync_current_provider_for_app_respecting_takeover(
    state: &AppState,
    app_type: &AppType,
) -> Result<(), AppError> {
    let current_id = match crate::settings::get_effective_current_provider(&state.db, app_type)? {
        Some(id) => id,
        None => return Ok(()),
    };

    let providers = state.db.get_all_providers(app_type.as_str())?;
    let Some(provider) = providers.get(&current_id) else {
        return Ok(());
    };

    let has_live_backup = futures::executor::block_on(state.db.get_live_backup(app_type.as_str()))
        .ok()
        .flatten()
        .is_some();
    let live_taken_over = state
        .proxy_service
        .detect_takeover_in_live_config_for_app(app_type);

    // `enabled` is set only after takeover writes complete. During that
    // activation window, backup/live placeholders are the authoritative signal
    // that normal provider sync must not rewrite the managed live file.
    if has_live_backup || live_taken_over {
        if !takeover_uses_live_backup(app_type) {
            write_live_with_common_config(state.db.as_ref(), app_type, provider)?;
        } else {
            futures::executor::block_on(
                state
                    .proxy_service
                    .update_live_backup_from_provider(app_type.as_str(), provider),
            )
            .map_err(|e| AppError::Message(format!("更新 Live 备份失败: {e}")))?;
        }
        return Ok(());
    }

    write_live_with_common_config(state.db.as_ref(), app_type, provider)
}

/// Sync current provider to live configuration
///
/// 使用有效的当前供应商 ID（验证过存在性）。
/// 优先从本地 settings 读取，验证后 fallback 到数据库的 is_current 字段。
/// 这确保了配置导入后无效 ID 会自动 fallback 到数据库。
///
/// For additive mode apps (OpenCode), all providers are synced instead of just the current one.
pub fn sync_current_to_live(state: &AppState) -> Result<(), AppError> {
    // Sync providers based on mode
    for app_type in AppType::all() {
        if app_type.is_additive_mode() {
            // Additive mode: sync ALL providers
            sync_all_providers_to_live(state, &app_type)?;
        } else {
            // Switch mode: sync only current provider. During proxy takeover,
            // update the restore backup instead of rewriting the taken-over
            // live file.
            sync_current_provider_for_app_respecting_takeover(state, &app_type)?;
        }
    }

    // MCP sync（best-effort 逐应用投影，内部已聚合失败）。错误暂存到
    // Skill 同步之后再返回：MCP 的失败不该跳过 Skill 同步，但调用方
    //（配置导入 / 云同步恢复）需要知道结果不完整。
    let mcp_result = McpService::sync_all_enabled(state);

    // Skill sync
    for app_type in AppType::all() {
        if let Err(e) = crate::services::skill::SkillService::sync_to_app(&state.db, &app_type) {
            log::warn!("同步 Skill 到 {app_type:?} 失败: {e}");
            // Continue syncing other apps, don't abort
        }
    }

    mcp_result
}

/// Read current live settings for an app type
pub fn read_live_settings(app_type: AppType) -> Result<Value, AppError> {
    crate::apps::adapter(&app_type).read_live()
}

/// Import default configuration from live files
///
/// Returns `Ok(true)` if a provider was actually imported,
/// `Ok(false)` if skipped (providers already exist for this app).
pub fn import_default_config(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
    // Additive mode apps (OpenCode, OpenClaw) should use their dedicated
    // import_xxx_providers_from_live functions, not this generic default config import
    if app_type.is_additive_mode() {
        return Ok(false);
    }

    // 允许 "只有官方 seed 预设" 的情况下继续导入 live：
    // - 启动编排顺序是先 import 后 seed，新用户启动时 providers 为空，导入照常
    // - 老用户已有非 seed provider，跳过导入（正确）
    // - 用户手动点 ProviderEmptyState 的导入按钮时，与官方 seed 共存而不被阻塞
    if state.db.has_non_official_seed_provider(app_type.as_str())? {
        return Ok(false);
    }

    // 拒绝把"被代理接管的 Live"导入为供应商：接管期间 Live 里只有
    // PROXY_MANAGED 占位符和本地代理地址，不是用户的真实配置。一旦导入，
    // 它会成为 current provider（SSOT），后续"无备份恢复"路径会把占位符
    // 当真实配置写回 Live，永久卡在已失效的本地代理上。
    // 典型触发场景：代理接管开启时切换 app_config_dir 并重启，新数据库首启导入。
    if state
        .proxy_service
        .detect_takeover_in_live_config_for_app(&app_type)
    {
        return Err(AppError::localized(
            "provider.import.live_taken_over",
            "Live 配置当前处于代理接管状态（包含占位符），不能导入为供应商。请先关闭代理接管或恢复 Live 配置后重试。",
            "The live config is currently taken over by the proxy (contains placeholders) and cannot be imported as a provider. Disable proxy takeover or restore the live config first.",
        ));
    }

    let adapter = crate::apps::adapter(&app_type);
    let settings_config = adapter.import_live()?;

    let mut provider = Provider::with_id(
        "default".to_string(),
        "default".to_string(),
        settings_config,
        None,
    );
    provider.category = Some(
        adapter
            .import_category(&provider.settings_config)
            .to_string(),
    );

    state.db.save_provider(app_type.as_str(), &provider)?;
    state
        .db
        .set_current_provider(app_type.as_str(), &provider.id)?;
    crate::settings::set_current_provider(&app_type, Some(provider.id.as_str()))?;

    Ok(true) // 真正导入了
}

/// Decide whether startup should auto-import the current live config as `default`.
///
/// This is intentionally stricter than the manual import path:
/// if the app already has any provider row at all (including official seeds),
/// startup must skip auto-import to avoid recreating `default` on each launch.
pub fn should_import_default_config_on_startup(
    state: &AppState,
    app_type: &AppType,
) -> Result<bool, AppError> {
    if app_type.is_additive_mode() {
        return Ok(false);
    }

    Ok(!state.db.has_any_provider_for_app(app_type.as_str())?)
}

/// Import all providers from OpenCode live config to database
///
/// This imports existing providers from ~/.config/opencode/opencode.json
/// into the CC Switch database. Each provider found will be added to the
/// database with is_current set to false.
pub fn import_opencode_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    use crate::opencode_config;

    let providers = opencode_config::get_typed_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let mut imported = 0;
    let mut updated = 0;
    let existing_ids = state.db.get_provider_ids("opencode")?;

    for (id, config) in providers {
        // Convert to Value for settings_config
        let settings_config = match serde_json::to_value(&config) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to serialize OpenCode provider '{id}': {e}");
                continue;
            }
        };

        if existing_ids.contains(&id) {
            match state.db.get_provider_by_id(&id, "opencode") {
                Ok(Some(existing)) => {
                    let display_name = config.name.clone().unwrap_or_else(|| existing.name.clone());
                    if existing.settings_config != settings_config || existing.name != display_name
                    {
                        let mut provider = existing;
                        provider.name = display_name;
                        provider.settings_config = settings_config;
                        if let Err(e) = state.db.save_provider("opencode", &provider) {
                            log::warn!(
                                "Failed to update OpenCode provider '{id}' from live config: {e}"
                            );
                        } else {
                            updated += 1;
                            log::info!("Updated OpenCode provider '{id}' from live config");
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("OpenCode provider '{id}' disappeared while importing live config")
                }
                Err(e) => log::warn!("Failed to look up OpenCode provider '{id}': {e}"),
            }
            continue;
        }

        // Create provider
        let display_name = config.name.clone().unwrap_or_else(|| id.clone());
        let mut provider = Provider::with_id(id.clone(), display_name, settings_config, None);
        provider.meta = Some(crate::provider::ProviderMeta {
            live_config_managed: Some(true),
            ..Default::default()
        });

        // Save to database
        if let Err(e) = state.db.save_provider("opencode", &provider) {
            log::warn!("Failed to import OpenCode provider '{id}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported OpenCode provider '{id}' from live config");
    }

    Ok(imported + updated)
}

/// Import all providers from OpenClaw live config to database
///
/// This imports existing providers from ~/.openclaw/openclaw.json
/// into the CC Switch database. Each provider found will be added to the
/// database with is_current set to false.
pub fn import_openclaw_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    use crate::openclaw_config;

    let providers = openclaw_config::get_typed_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let mut imported = 0;
    let mut updated = 0;
    let existing_ids = state.db.get_provider_ids("openclaw")?;

    for (id, config) in providers {
        // Validate: skip entries with empty id or no models
        if id.trim().is_empty() {
            log::warn!("Skipping OpenClaw provider with empty id");
            continue;
        }
        if config.models.is_empty() {
            log::warn!("Skipping OpenClaw provider '{id}': no models defined");
            continue;
        }

        // Convert to Value for settings_config
        let settings_config = match serde_json::to_value(&config) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to serialize OpenClaw provider '{id}': {e}");
                continue;
            }
        };

        if existing_ids.contains(&id) {
            match state.db.get_provider_by_id(&id, "openclaw") {
                Ok(Some(existing)) => {
                    if existing.settings_config != settings_config {
                        let mut provider = existing;
                        provider.settings_config = settings_config;
                        if let Err(e) = state.db.save_provider("openclaw", &provider) {
                            log::warn!(
                                "Failed to update OpenClaw provider '{id}' from live config: {e}"
                            );
                        } else {
                            updated += 1;
                            log::info!("Updated OpenClaw provider '{id}' from live config");
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("OpenClaw provider '{id}' disappeared while importing live config")
                }
                Err(e) => log::warn!("Failed to look up OpenClaw provider '{id}': {e}"),
            }
            continue;
        }

        // Determine display name: use first model name if available, otherwise use id
        let display_name = config
            .models
            .first()
            .and_then(|m| m.name.clone())
            .unwrap_or_else(|| id.clone());

        // Create provider
        let mut provider = Provider::with_id(id.clone(), display_name, settings_config, None);
        provider.meta = Some(crate::provider::ProviderMeta {
            live_config_managed: Some(true),
            ..Default::default()
        });

        // Save to database
        if let Err(e) = state.db.save_provider("openclaw", &provider) {
            log::warn!("Failed to import OpenClaw provider '{id}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported OpenClaw provider '{id}' from live config");
    }

    Ok(imported + updated)
}

/// Import all providers from Hermes live config to database
///
/// This imports existing providers from ~/.hermes/config.yaml
/// into the CC Switch database. Each provider found will be added to the
/// database with is_current set to false.
pub fn import_hermes_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    use crate::hermes_config;

    let providers = hermes_config::get_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let mut imported = 0;
    let mut updated = 0;
    let existing_ids = state.db.get_provider_ids("hermes")?;

    for (name, config) in providers {
        // Validate: skip entries with empty name
        if name.trim().is_empty() {
            log::warn!("Skipping Hermes provider with empty name");
            continue;
        }

        if existing_ids.contains(&name) {
            match state.db.get_provider_by_id(&name, "hermes") {
                Ok(Some(existing)) => {
                    if existing.settings_config != config {
                        let mut provider = existing;
                        provider.settings_config = config;
                        if let Err(e) = state.db.save_provider("hermes", &provider) {
                            log::warn!(
                                "Failed to update Hermes provider '{name}' from live config: {e}"
                            );
                        } else {
                            updated += 1;
                            log::info!("Updated Hermes provider '{name}' from live config");
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("Hermes provider '{name}' disappeared while importing live config")
                }
                Err(e) => log::warn!("Failed to look up Hermes provider '{name}': {e}"),
            }
            continue;
        }

        // Create provider
        let mut provider = Provider::with_id(name.clone(), name.clone(), config, None);
        provider.meta = Some(crate::provider::ProviderMeta {
            live_config_managed: Some(true),
            ..Default::default()
        });

        // Save to database
        if let Err(e) = state.db.save_provider("hermes", &provider) {
            log::warn!("Failed to import Hermes provider '{name}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported Hermes provider '{name}' from live config");
    }

    Ok(imported + updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use toml_edit::DocumentMut;

    #[test]
    fn kimi_for_coding_effective_settings_backfill_256k_context() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "kimi-for-coding".to_string(),
            "Kimi For Coding".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/",
                    "ANTHROPIC_MODEL": "kimi-for-coding",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144"
                }
            }),
            None,
        );

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("262144")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("262144")
        );
    }

    #[test]
    fn kimi_for_coding_context_defaults_preserve_user_overrides() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "kimi-for-coding".to_string(),
            "Kimi For Coding".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding",
                    "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "300000",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "250000"
                }
            }),
            None,
        );

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("300000")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("250000")
        );
    }

    #[test]
    fn kimi_for_coding_backfill_strips_only_injected_context_default() {
        let db = Database::memory().expect("create memory db");
        let provider = Provider::with_id(
            "kimi-for-coding".to_string(),
            "Kimi For Coding".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding/",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144"
                }
            }),
            None,
        );

        let live = build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
            .expect("build effective settings");
        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .is_none());
        assert_eq!(
            backfilled["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("262144")
        );
    }

    #[test]
    fn codex_oauth_effective_settings_backfill_gpt_context_defaults() {
        let db = Database::memory().expect("create memory db");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "gpt-5.6"
                }
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("372000")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("372000")
        );
    }

    #[test]
    fn codex_oauth_context_defaults_preserve_user_overrides() {
        let db = Database::memory().expect("create memory db");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "500000",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "350000"
                }
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("500000")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("350000")
        );
    }

    #[test]
    fn codex_oauth_context_defaults_ignore_legacy_common_config_values() {
        let db = Database::memory().expect("create memory db");
        db.set_config_snippet(
            AppType::Claude.as_str(),
            Some(
                json!({
                    "env": {
                        "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144",
                        "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144"
                    }
                })
                .to_string(),
            ),
        )
        .expect("save legacy common config");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({ "env": { "ANTHROPIC_MODEL": "gpt-5.6" } }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            common_config_enabled: Some(true),
            ..Default::default()
        });

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        assert_eq!(
            effective["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("372000")
        );
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("372000")
        );
    }

    #[test]
    fn codex_oauth_context_defaults_skip_non_gpt56_models() {
        let db = Database::memory().expect("create memory db");
        db.set_config_snippet(
            AppType::Claude.as_str(),
            Some(
                json!({
                    "env": {
                        "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "262144",
                        "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "262144"
                    }
                })
                .to_string(),
            ),
        )
        .expect("save legacy common config");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "gpt-5.5",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "300000"
                }
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            common_config_enabled: Some(true),
            ..Default::default()
        });

        let effective =
            build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
                .expect("build effective settings");
        // 旧模型不注入 372K 默认值，遗留共享片段带进来的值也要剥掉
        assert!(effective["env"]
            .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .is_none());
        // 用户显式写在供应商配置里的值仍然生效
        assert_eq!(
            effective["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("300000")
        );
    }

    /// 往返不动点：注入产物只活在 live，切走回灌后存储配置必须与注入前一致，
    /// 否则程序默认值固化成"用户显式值"，之后调默认值永远压不动。
    #[test]
    fn codex_oauth_backfill_strips_injected_context_defaults() {
        let db = Database::memory().expect("create memory db");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({ "env": { "ANTHROPIC_MODEL": "gpt-5.6" } }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        // 模拟写 live：注入了两个上下文默认值
        let live = build_effective_settings_with_common_config(&db, &AppType::Claude, &provider)
            .expect("build effective settings");
        assert_eq!(
            live["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("372000")
        );

        // 模拟切走回灌：注入产物被剥掉，其余字段原样保留
        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .is_none());
        assert!(backfilled["env"]
            .get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
            .is_none());
        assert_eq!(backfilled["env"]["ANTHROPIC_MODEL"], json!("gpt-5.6"));
    }

    #[test]
    fn codex_oauth_backfill_keeps_user_context_values() {
        let db = Database::memory().expect("create memory db");
        let mut provider = Provider::with_id(
            "codex-oauth".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "gpt-5.6",
                    "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "500000"
                }
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        // live 里：MAX 是用户显式值；ACW 被用户手改成了非默认数字
        let live = json!({
            "env": {
                "ANTHROPIC_MODEL": "gpt-5.6",
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "500000",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "300000"
            }
        });
        let backfilled =
            strip_common_config_from_live_settings(&db, &AppType::Claude, &provider, live);
        assert_eq!(
            backfilled["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("500000")
        );
        assert_eq!(
            backfilled["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("300000")
        );
    }

    /// C5 回归锁：前端表单的合并/剥离必须走 toml_edit 文档模型。
    /// smol-toml 的 parse→merge→stringify 整文档重序列化会丢注释、
    /// 按字母序重排键、并为 dotted 表生成多余的空父表头。
    #[test]
    fn update_toml_common_config_snippet_preserves_comments_and_key_order() {
        // 刻意非字母序的键序 + 注释，模拟用户手写格式
        let config = r#"# my precious comment
model = "gpt-5.5"
model_provider = "aprov"
disable_response_storage = true

[model_providers.aprov]
# provider comment
name = "A Prov"
base_url = "https://a.example/v1"
"#;
        let snippet = "[tui]\nnotifications = true\n";

        let merged = update_toml_common_config_snippet(config, snippet, true).unwrap();
        assert!(merged.contains("# my precious comment"));
        assert!(merged.contains("# provider comment"));
        let model_pos = merged.find("model = ").unwrap();
        let provider_pos = merged.find("model_provider = ").unwrap();
        let disable_pos = merged.find("disable_response_storage").unwrap();
        assert!(
            model_pos < provider_pos && provider_pos < disable_pos,
            "merge must not reorder user keys, got: {merged}"
        );
        assert!(merged.contains("[tui]"));
        assert!(merged.contains("notifications = true"));
        assert!(
            !merged.contains("[model_providers]\n"),
            "merge must not synthesize an empty parent table header, got: {merged}"
        );

        let removed = update_toml_common_config_snippet(&merged, snippet, false).unwrap();
        assert!(!removed.contains("[tui]"), "snippet keys must be stripped");
        assert!(removed.contains("# my precious comment"));
        assert!(removed.contains("disable_response_storage = true"));
    }

    /// 合并时标量=片段覆盖供应商值（与 Claude 侧 deepMerge 一致）；
    /// 剥离按值匹配：用户改过的值不删（与 strip 路径的
    /// toml_value_is_subset 语义一致）。
    #[test]
    fn update_toml_common_config_snippet_scalar_override_and_value_matched_removal() {
        let snippet = "[tui]\nnotifications = true\n";

        let merged =
            update_toml_common_config_snippet("[tui]\nnotifications = false\n", snippet, true)
                .unwrap();
        assert!(
            merged.contains("notifications = true"),
            "snippet scalar should override provider value, got: {merged}"
        );

        let removed =
            update_toml_common_config_snippet("[tui]\nnotifications = false\n", snippet, false)
                .unwrap();
        assert!(
            removed.contains("notifications = false"),
            "user-modified value must survive removal, got: {removed}"
        );
    }

    #[test]
    fn claude_common_config_apply_and_remove_roundtrip_for_non_overlapping_fields() {
        let settings = json!({
            "env": {
                "ANTHROPIC_API_KEY": "sk-test"
            }
        });
        let snippet = r#"{
  "includeCoAuthoredBy": false,
  "env": {
    "CLAUDE_CODE_USE_BEDROCK": "1"
  }
}"#;

        let applied =
            apply_common_config_to_settings(&AppType::Claude, &settings, snippet).unwrap();
        assert_eq!(applied["includeCoAuthoredBy"], json!(false));
        assert_eq!(applied["env"]["CLAUDE_CODE_USE_BEDROCK"], json!("1"));

        let stripped =
            remove_common_config_from_settings(&AppType::Claude, &applied, snippet).unwrap();
        assert_eq!(stripped, settings);
    }

    #[test]
    fn codex_common_config_apply_and_remove_roundtrip_for_non_overlapping_fields() {
        let settings = json!({
            "auth": {
                "OPENAI_API_KEY": "sk-test"
            },
            "config": "model_provider = \"openai\"\n[general]\nmodel = \"gpt-5\"\n"
        });
        let snippet = "[shared]\nreasoning = \"medium\"\n";

        let applied = apply_common_config_to_settings(&AppType::Codex, &settings, snippet).unwrap();
        let applied_config = applied["config"].as_str().unwrap_or_default();
        assert!(applied_config.contains("[shared]"));
        assert!(applied_config.contains("reasoning = \"medium\""));

        let stripped =
            remove_common_config_from_settings(&AppType::Codex, &applied, snippet).unwrap();
        assert_eq!(stripped, settings);
    }

    #[test]
    fn explicit_common_config_flag_overrides_legacy_subset_detection() {
        let mut provider = Provider::with_id(
            "claude-test".to_string(),
            "Claude Test".to_string(),
            json!({
                "includeCoAuthoredBy": false
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            common_config_enabled: Some(false),
            ..Default::default()
        });

        assert!(
            !provider_uses_common_config(
                &AppType::Claude,
                &provider,
                Some(r#"{ "includeCoAuthoredBy": false }"#),
            ),
            "explicit false should win over legacy subset detection"
        );
    }

    #[test]
    fn claude_common_config_array_subset_detection_and_strip_preserve_extra_items() {
        let settings = json!({
            "allowedTools": ["tool1", "tool2"]
        });
        let snippet = r#"{
  "allowedTools": ["tool1"]
}"#;

        assert!(
            settings_contain_common_config(&AppType::Claude, &settings, snippet),
            "array subset should be detected for legacy providers"
        );

        let stripped =
            remove_common_config_from_settings(&AppType::Claude, &settings, snippet).unwrap();
        assert_eq!(
            stripped,
            json!({
                "allowedTools": ["tool2"]
            })
        );
    }

    #[test]
    fn codex_common_config_array_subset_detection_and_strip_preserve_extra_items() {
        let settings = json!({
            "auth": {},
            "config": "allowed_tools = [\"tool1\", \"tool2\"]\n"
        });
        let snippet = "allowed_tools = [\"tool1\"]\n";

        assert!(
            settings_contain_common_config(&AppType::Codex, &settings, snippet),
            "TOML array subset should be detected for legacy providers"
        );

        let stripped =
            remove_common_config_from_settings(&AppType::Codex, &settings, snippet).unwrap();
        assert_eq!(stripped["auth"], json!({}));
        let stripped_config = stripped["config"].as_str().unwrap_or_default();
        let parsed = stripped_config
            .parse::<DocumentMut>()
            .expect("stripped codex config should remain valid TOML");
        let allowed_tools = parsed["allowed_tools"]
            .as_array()
            .expect("allowed_tools should remain an array");
        let values: Vec<&str> = allowed_tools
            .iter()
            .map(|value| value.as_str().expect("tool id should be string"))
            .collect();
        assert_eq!(values, vec!["tool2"]);
    }

    #[test]
    fn codex_switch_backfill_preserves_stored_model_catalog_when_live_lacks_it() {
        // Reproduces the data-loss bug: switching away from a Codex provider
        // backfills the outgoing provider from Live, but Live's config.toml had
        // already lost its `model_catalog_json` projection (proxy cycle /
        // Codex.app rewrite), so `read_live_settings` reconstructs no catalog.
        // The stored mapping must survive the backfill.
        let mut provider = Provider::with_id(
            "deepseek".to_string(),
            "DeepSeek".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-deepseek" },
                "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-pro\"\n",
                "modelCatalog": {
                    "models": [
                        { "model": "deepseek-v4-pro", "contextWindow": 1_000_000 }
                    ]
                }
            }),
            None,
        );
        provider.category = Some("cn_official".to_string());

        // Live snapshot as captured during switch: no `modelCatalog` field.
        let live_settings = json!({
            "auth": { "OPENAI_API_KEY": "sk-deepseek" },
            "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-pro\"\n"
        });

        let result =
            restore_live_settings_for_provider_backfill(&AppType::Codex, &provider, live_settings);

        assert_eq!(
            result.get("modelCatalog"),
            provider.settings_config.get("modelCatalog"),
            "switch-away backfill must keep the DB-stored modelCatalog when Live has none"
        );
    }

    #[test]
    fn codex_switch_backfill_keeps_live_catalog_when_db_has_none() {
        // When the DB provider has no stored catalog, a catalog reconstructed
        // from Live (if any) should be left intact — the DB-preference overlay
        // must not wipe it.
        let mut provider = Provider::with_id(
            "deepseek".to_string(),
            "DeepSeek".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-deepseek" },
                "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-pro\"\n"
            }),
            None,
        );
        provider.category = Some("cn_official".to_string());

        let live_settings = json!({
            "auth": { "OPENAI_API_KEY": "sk-deepseek" },
            "config": "model_provider = \"custom\"\nmodel = \"deepseek-v4-pro\"\n",
            "modelCatalog": { "models": [ { "model": "deepseek-v4-pro" } ] }
        });

        let result = restore_live_settings_for_provider_backfill(
            &AppType::Codex,
            &provider,
            live_settings.clone(),
        );

        assert_eq!(
            result.get("modelCatalog"),
            live_settings.get("modelCatalog"),
            "backfill must keep the Live-reconstructed catalog when the DB has none"
        );
    }

    #[test]
    fn codex_switch_backfill_strips_synced_mcp_servers() {
        // Live 里的 [mcp_servers] 是 MCP 同步的投影（SSOT 在 DB 表），
        // 回填进供应商存储配置会让已删除的服务器随快照复活。
        let provider = Provider::with_id(
            "prov".to_string(),
            "Prov".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-test" },
                "config": "model = \"gpt-5.5\"\n"
            }),
            None,
        );

        let live_settings = json!({
            "auth": { "OPENAI_API_KEY": "sk-test" },
            "config": "model = \"gpt-5.5\"\n\n[mcp_servers.echo]\ntype = \"stdio\"\ncommand = \"echo\"\n"
        });

        let result =
            restore_live_settings_for_provider_backfill(&AppType::Codex, &provider, live_settings);

        let config_text = result
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config text");
        assert!(
            !config_text.contains("mcp_servers"),
            "backfill must strip synced [mcp_servers] from the stored provider config, got: {config_text}"
        );
        assert!(
            config_text.contains("model = \"gpt-5.5\""),
            "non-MCP content must survive the strip"
        );
    }

    #[test]
    fn grok_switch_backfill_strips_synced_mcp_servers() {
        let provider = Provider::with_id(
            "grok".to_string(),
            "Grok".to_string(),
            json!({
                "config": "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"https://example.com/v1\"\nname = \"Example\"\napi_key = \"secret\"\napi_backend = \"responses\"\ncontext_window = 500000\n"
            }),
            None,
        );
        let live_settings = json!({
            "config": "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"https://example.com/v1\"\nname = \"Example\"\napi_key = \"secret\"\napi_backend = \"responses\"\ncontext_window = 500000\n\n[mcp_servers.echo]\ncommand = \"echo\"\n"
        });

        let result = restore_live_settings_for_provider_backfill(
            &AppType::GrokBuild,
            &provider,
            live_settings,
        );
        let config_text = result
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");

        assert!(!config_text.contains("mcp_servers"));
        assert!(config_text.contains("model = \"grok-4.5\""));
    }
}
