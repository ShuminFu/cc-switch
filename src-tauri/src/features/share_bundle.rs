//! Share-safe export and import preview (PRODUCT-22).
//!
//! A share bundle is a JSON file (`format: "cc-switch-share"`) with a chosen
//! scope: providers of selected apps, MCP servers and prompts. Secrets are
//! stripped by default (blank strings; see [`super::redact::strip`]), so a
//! bundle can go to a teammate or a wiki. The full-database SQL export stays
//! the way to make a backup.
//!
//! Import never replaces the database: [`preview_share_bundle`] reads the file
//! and classifies every item as new, identical or conflicting (same id) and
//! lists the secrets the recipient must supply; [`apply_share_bundle`] then
//! merges only the selected items. Blank secrets of an overwritten item keep
//! the local value, so importing a stripped bundle never wipes a key. New
//! providers are added without being activated and prompts are imported
//! disabled, so no live tool config changes unless an existing item that is
//! already active is overwritten.

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use super::audit_log::{self, AuditEvent};
use super::redact;
use crate::app_config::{AppType, McpServer};
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::provider::Provider;
use crate::store::AppState;

pub const FORMAT: &str = "cc-switch-share";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleProvider {
    pub app: String,
    pub provider: Provider,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundlePrompt {
    pub app: String,
    pub prompt: Prompt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareBundle {
    pub format: String,
    pub version: u32,
    pub created_at: String,
    pub secrets_stripped: bool,
    #[serde(default)]
    pub providers: Vec<BundleProvider>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
    #[serde(default)]
    pub prompts: Vec<BundlePrompt>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportScope {
    /// Apps whose providers are exported.
    #[serde(default)]
    pub provider_apps: Vec<String>,
    #[serde(default)]
    pub mcp: bool,
    /// Apps whose prompts are exported.
    #[serde(default)]
    pub prompt_apps: Vec<String>,
    #[serde(default = "default_true")]
    pub strip_secrets: bool,
}

fn default_true() -> bool {
    true
}

fn parse_app(app: &str) -> Result<AppType, AppError> {
    AppType::from_str(app).map_err(|e| AppError::Message(e.to_string()))
}

fn to_value<T: Serialize>(v: &T) -> Result<Value, AppError> {
    serde_json::to_value(v).map_err(|source| AppError::JsonSerialize { source })
}

fn from_value<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, AppError> {
    serde_json::from_value(v).map_err(|e| AppError::Message(format!("Invalid bundle item: {e}")))
}

fn maybe_strip<T: Serialize + serde::de::DeserializeOwned>(
    item: &T,
    strip: bool,
) -> Result<(T, usize), AppError> {
    if !strip {
        return Ok((from_value(to_value(item)?)?, 0));
    }
    let (v, n) = redact::strip(&to_value(item)?);
    Ok((from_value(v)?, n))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub path: Option<String>,
    pub providers: usize,
    pub mcp_servers: usize,
    pub prompts: usize,
    pub secrets_stripped: usize,
}

fn build_bundle(state: &AppState, scope: &ExportScope) -> Result<(ShareBundle, usize), AppError> {
    let mut stripped = 0;
    let mut providers = Vec::new();
    for app in &scope.provider_apps {
        let app_type = parse_app(app)?;
        for (_, p) in state.db.get_all_providers(app_type.as_str())? {
            let (p, n) = maybe_strip(&p, scope.strip_secrets)?;
            stripped += n;
            providers.push(BundleProvider {
                app: app_type.as_str().to_string(),
                provider: p,
            });
        }
    }
    let mut mcp_servers = Vec::new();
    if scope.mcp {
        for (_, s) in state.db.get_all_mcp_servers()? {
            let (s, n) = maybe_strip(&s, scope.strip_secrets)?;
            stripped += n;
            mcp_servers.push(s);
        }
    }
    let mut prompts = Vec::new();
    for app in &scope.prompt_apps {
        let app_type = parse_app(app)?;
        for (_, p) in state.db.get_prompts(app_type.as_str())? {
            prompts.push(BundlePrompt {
                app: app_type.as_str().to_string(),
                prompt: Prompt {
                    enabled: false,
                    ..p
                },
            });
        }
    }
    Ok((
        ShareBundle {
            format: FORMAT.to_string(),
            version: VERSION,
            created_at: chrono::Utc::now().to_rfc3339(),
            secrets_stripped: scope.strip_secrets,
            providers,
            mcp_servers,
            prompts,
        },
        stripped,
    ))
}

fn parse_bundle(text: &str) -> Result<ShareBundle, AppError> {
    let bundle: ShareBundle = serde_json::from_str(text)
        .map_err(|e| AppError::Message(format!("Not a cc-switch share bundle: {e}")))?;
    if bundle.format != FORMAT {
        return Err(AppError::Message(format!(
            "Not a cc-switch share bundle (format '{}')",
            bundle.format
        )));
    }
    if bundle.version > VERSION {
        return Err(AppError::Message(format!(
            "This bundle was made by a newer cc-switch (version {})",
            bundle.version
        )));
    }
    for p in &bundle.providers {
        parse_app(&p.app)?;
    }
    for p in &bundle.prompts {
        parse_app(&p.app)?;
    }
    Ok(bundle)
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ItemStatus {
    New,
    Identical,
    Conflict,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    /// `provider:<app>:<id>`, `mcp:<id>` or `prompt:<app>:<id>`.
    pub key: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub id: String,
    pub name: String,
    pub status: ItemStatus,
    /// Blank secret fields (JSON pointers into the item). Fields the local
    /// copy already has are not listed for conflicts.
    pub missing_secrets: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundlePreview {
    pub created_at: String,
    pub secrets_stripped: bool,
    pub items: Vec<PreviewItem>,
}

/// Comparable form: secrets stripped on both sides, so a stripped bundle of
/// an unchanged item reads as identical.
fn comparable(v: &Value) -> String {
    redact::canonical_hash(&redact::strip(v).0)
}

fn missing_after_fill(item: &Value, local: Option<&Value>) -> Vec<String> {
    redact::missing_secret_pointers(item)
        .into_iter()
        .filter(|ptr| {
            local
                .and_then(|l| l.pointer(ptr))
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        })
        .collect()
}

fn classify(item: &Value, local: Option<&Value>) -> ItemStatus {
    match local {
        None => ItemStatus::New,
        Some(l) if comparable(l) == comparable(item) => ItemStatus::Identical,
        Some(_) => ItemStatus::Conflict,
    }
}

fn preview(state: &AppState, bundle: &ShareBundle) -> Result<BundlePreview, AppError> {
    let mut items = Vec::new();
    for bp in &bundle.providers {
        let app = parse_app(&bp.app)?;
        let item = to_value(&bp.provider)?;
        let local = state
            .db
            .get_provider_by_id(&bp.provider.id, app.as_str())?
            .map(|p| to_value(&p))
            .transpose()?;
        items.push(PreviewItem {
            key: format!("provider:{}:{}", app.as_str(), bp.provider.id),
            kind: "provider".into(),
            app: Some(app.as_str().to_string()),
            id: bp.provider.id.clone(),
            name: bp.provider.name.clone(),
            status: classify(&item, local.as_ref()),
            missing_secrets: missing_after_fill(&item, local.as_ref()),
        });
    }
    let servers = state.db.get_all_mcp_servers()?;
    for s in &bundle.mcp_servers {
        let item = to_value(s)?;
        let local = servers.get(&s.id).map(to_value).transpose()?;
        items.push(PreviewItem {
            key: format!("mcp:{}", s.id),
            kind: "mcp".into(),
            app: None,
            id: s.id.clone(),
            name: s.name.clone(),
            status: classify(&item, local.as_ref()),
            missing_secrets: missing_after_fill(&item, local.as_ref()),
        });
    }
    let mut prompt_cache: HashMap<String, indexmap::IndexMap<String, Prompt>> = HashMap::new();
    for bp in &bundle.prompts {
        let app = parse_app(&bp.app)?;
        let local_prompts = match prompt_cache.get(app.as_str()) {
            Some(p) => p,
            None => {
                let p = state.db.get_prompts(app.as_str())?;
                prompt_cache.entry(app.as_str().to_string()).or_insert(p)
            }
        };
        // `enabled` is device state, not content.
        let norm = |p: &Prompt| {
            to_value(&Prompt {
                enabled: false,
                created_at: None,
                updated_at: None,
                ..p.clone()
            })
        };
        let item = norm(&bp.prompt)?;
        let local = local_prompts.get(&bp.prompt.id).map(norm).transpose()?;
        items.push(PreviewItem {
            key: format!("prompt:{}:{}", app.as_str(), bp.prompt.id),
            kind: "prompt".into(),
            app: Some(app.as_str().to_string()),
            id: bp.prompt.id.clone(),
            name: bp.prompt.name.clone(),
            status: classify(&item, local.as_ref()),
            missing_secrets: Vec::new(),
        });
    }
    Ok(BundlePreview {
        created_at: bundle.created_at.clone(),
        secrets_stripped: bundle.secrets_stripped,
        items,
    })
}

/// Fill blank secrets: first from what the user typed in, then from the
/// local copy.
fn fill_secrets(item: &mut Value, local: Option<&Value>, typed: Option<&BTreeMap<String, String>>) {
    for ptr in redact::missing_secret_pointers(item) {
        let value = typed
            .and_then(|t| t.get(&ptr))
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.trim().to_string())
            .or_else(|| {
                local
                    .and_then(|l| l.pointer(&ptr))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            });
        if let (Some(v), Some(slot)) = (value, item.pointer_mut(&ptr)) {
            *slot = Value::String(v);
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    /// Item keys from the preview to import.
    pub keys: Vec<String>,
    /// Replace conflicting items (otherwise they are skipped).
    #[serde(default)]
    pub overwrite: bool,
    /// Item key → (JSON pointer → secret value) for missing secrets.
    #[serde(default)]
    pub secrets: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

fn apply(state: &AppState, bundle: &ShareBundle, req: &ApplyRequest) -> ApplyResult {
    let mut result = ApplyResult::default();
    let wanted = |key: &str| req.keys.iter().any(|k| k == key);
    for bp in &bundle.providers {
        let key = format!("provider:{}:{}", bp.app, bp.provider.id);
        if !wanted(&key) {
            continue;
        }
        let outcome = (|| -> Result<Option<bool>, AppError> {
            let app = parse_app(&bp.app)?;
            let local = state.db.get_provider_by_id(&bp.provider.id, app.as_str())?;
            let local_v = local.as_ref().map(to_value).transpose()?;
            let mut item = to_value(&bp.provider)?;
            if local.is_some()
                && (!req.overwrite || classify(&item, local_v.as_ref()) == ItemStatus::Identical)
            {
                return Ok(None);
            }
            fill_secrets(&mut item, local_v.as_ref(), req.secrets.get(&key));
            let provider: Provider = from_value(item)?;
            let id = provider.id.clone();
            let exists = local.is_some();
            super::config_history::track_provider_change(state, &app, &id, &id, "import", || {
                if exists {
                    crate::services::ProviderService::update(state, app.clone(), None, provider)
                } else {
                    crate::services::ProviderService::add(state, app.clone(), provider, false)
                }
            })?;
            Ok(Some(exists))
        })();
        tally(&mut result, &key, outcome);
    }
    let servers = state.db.get_all_mcp_servers().unwrap_or_default();
    for s in &bundle.mcp_servers {
        let key = format!("mcp:{}", s.id);
        if !wanted(&key) {
            continue;
        }
        let outcome = (|| -> Result<Option<bool>, AppError> {
            let local_v = servers.get(&s.id).map(to_value).transpose()?;
            let mut item = to_value(s)?;
            if local_v.is_some()
                && (!req.overwrite || classify(&item, local_v.as_ref()) == ItemStatus::Identical)
            {
                return Ok(None);
            }
            fill_secrets(&mut item, local_v.as_ref(), req.secrets.get(&key));
            let server: McpServer = from_value(item)?;
            let before = local_v.clone();
            crate::services::McpService::upsert_server(state, server.clone())?;
            audit_log::record(
                AuditEvent::new("import", "mcp.upsert", "mcp", &server.id)
                    .states(before.as_ref(), to_value(&server).ok().as_ref()),
            );
            Ok(Some(local_v.is_some()))
        })();
        tally(&mut result, &key, outcome);
    }
    for bp in &bundle.prompts {
        let key = format!("prompt:{}:{}", bp.app, bp.prompt.id);
        if !wanted(&key) {
            continue;
        }
        let outcome = (|| -> Result<Option<bool>, AppError> {
            let app = parse_app(&bp.app)?;
            let local = state
                .db
                .get_prompts(app.as_str())?
                .shift_remove(&bp.prompt.id);
            if local.is_some() && !req.overwrite {
                return Ok(None);
            }
            let now = chrono::Utc::now().timestamp();
            let prompt = Prompt {
                enabled: local.as_ref().is_some_and(|l| l.enabled),
                created_at: local.as_ref().and_then(|l| l.created_at).or(Some(now)),
                updated_at: Some(now),
                ..bp.prompt.clone()
            };
            if local
                .as_ref()
                .is_some_and(|l| l.content == prompt.content && l.name == prompt.name)
            {
                return Ok(None);
            }
            crate::services::PromptService::upsert_prompt(
                state,
                app.clone(),
                &prompt.id,
                prompt.clone(),
            )?;
            audit_log::record(
                AuditEvent::new("import", "prompt.upsert", "prompt", &prompt.id).app(app.as_str()),
            );
            Ok(Some(local.is_some()))
        })();
        tally(&mut result, &key, outcome);
    }
    result
}

fn tally(result: &mut ApplyResult, key: &str, outcome: Result<Option<bool>, AppError>) {
    match outcome {
        Ok(None) => result.skipped += 1,
        Ok(Some(true)) => result.updated += 1,
        Ok(Some(false)) => result.added += 1,
        Err(e) => result.errors.push(format!("{key}: {e}")),
    }
}

/// The bundle read by the last preview, applied by [`apply_share_bundle`].
/// Kept in the backend so secrets in an unstripped bundle never pass through
/// the webview.
static PENDING: Mutex<Option<ShareBundle>> = Mutex::new(None);

#[tauri::command]
pub async fn export_share_bundle(
    handle: AppHandle,
    scope: ExportScope,
) -> Result<ExportSummary, String> {
    let (bundle, stripped) =
        crate::commands::run_blocking_with_state(handle.clone(), move |state| {
            build_bundle(state, &scope).map_err(|e| e.to_string())
        })
        .await?;
    let text = serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())?;
    let name = format!(
        "cc-switch-share-{}.json",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let summary = ExportSummary {
        path: None,
        providers: bundle.providers.len(),
        mcp_servers: bundle.mcp_servers.len(),
        prompts: bundle.prompts.len(),
        secrets_stripped: stripped,
    };
    let path = tauri::async_runtime::spawn_blocking(move || {
        super::file_dialog::save_text(&handle, &name, "cc-switch share bundle", "json", &text)
    })
    .await
    .map_err(|e| e.to_string())??;
    if path.is_some() {
        audit_log::record(
            AuditEvent::new("ui", "bundle.export", "bundle", "share")
                .detail(format!("secretsStripped={}", bundle.secrets_stripped)),
        );
    }
    Ok(ExportSummary { path, ..summary })
}

/// Pick a bundle file and preview what importing it would do. `None` when
/// the dialog was cancelled.
#[tauri::command]
pub async fn preview_share_bundle(handle: AppHandle) -> Result<Option<BundlePreview>, String> {
    let dialog_handle = handle.clone();
    let text = tauri::async_runtime::spawn_blocking(move || {
        super::file_dialog::open_text(&dialog_handle, "cc-switch share bundle", &["json"])
    })
    .await
    .map_err(|e| e.to_string())??;
    let Some(text) = text else {
        return Ok(None);
    };
    let bundle = parse_bundle(&text).map_err(|e| e.to_string())?;
    let preview = crate::commands::run_blocking_with_state(handle, {
        let bundle = bundle.clone();
        move |state| preview(state, &bundle).map_err(|e| e.to_string())
    })
    .await?;
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(bundle);
    Ok(Some(preview))
}

#[tauri::command]
pub async fn apply_share_bundle(
    handle: AppHandle,
    request: ApplyRequest,
) -> Result<ApplyResult, String> {
    let bundle = PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or_else(|| "No bundle is being imported; preview it again".to_string())?;
    crate::commands::run_blocking_with_state(handle, move |state| {
        Ok::<_, String>(apply(state, &bundle, &request))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn state() -> AppState {
        AppState::new(Arc::new(crate::database::Database::memory().unwrap()))
    }

    fn claude(id: &str, key: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            format!("Relay {id}"),
            json!({ "env": {
                "ANTHROPIC_BASE_URL": "https://relay.example.com",
                "ANTHROPIC_AUTH_TOKEN": key
            } }),
            None,
        )
    }

    fn mcp(id: &str, token: &str) -> McpServer {
        from_value(json!({
            "id": id,
            "name": id,
            "server": { "type": "stdio", "command": "npx", "args": ["-y", "server"], "env": { "GITHUB_TOKEN": token } },
            "apps": { "claude": false, "codex": false, "gemini": false, "opencode": false }
        }))
        .unwrap()
    }

    fn all_scope() -> ExportScope {
        ExportScope {
            provider_apps: vec!["claude".into()],
            mcp: true,
            prompt_apps: vec!["claude".into()],
            strip_secrets: true,
        }
    }

    #[test]
    fn stripped_export_contains_no_secrets() {
        let _home = crate::test_support::isolated_home();
        let s = state();
        s.db.save_provider("claude", &claude("a", "sk-planted-secret-1"))
            .unwrap();
        s.db.save_mcp_server(&mcp("gh", "ghp_plantedtoken"))
            .unwrap();
        let (bundle, stripped) = build_bundle(&s, &all_scope()).unwrap();
        let text = serde_json::to_string(&bundle).unwrap();
        assert!(!text.contains("sk-planted-secret-1"));
        assert!(!text.contains("ghp_plantedtoken"));
        assert!(text.contains("relay.example.com"));
        assert_eq!(stripped, 2);
        assert!(parse_bundle(&text).is_ok());
        assert!(parse_bundle(
            "{\"format\":\"other\",\"version\":1,\"createdAt\":\"\",\"secretsStripped\":true}"
        )
        .is_err());
    }

    #[test]
    fn import_on_a_second_machine_asks_only_for_missing_keys() {
        let _home = crate::test_support::isolated_home();
        let source = state();
        source
            .db
            .save_provider("claude", &claude("a", "sk-1"))
            .unwrap();
        source
            .db
            .save_provider("claude", &claude("b", "sk-2"))
            .unwrap();
        let (bundle, _) = build_bundle(&source, &all_scope()).unwrap();

        let target = state();
        // `b` already exists with its own key; `a` is new.
        target
            .db
            .save_provider("claude", &claude("b", "sk-local"))
            .unwrap();
        let p = preview(&target, &bundle).unwrap();
        let a = p.items.iter().find(|i| i.id == "a").unwrap();
        let b = p.items.iter().find(|i| i.id == "b").unwrap();
        assert_eq!(a.status, ItemStatus::New);
        assert_eq!(
            a.missing_secrets,
            vec!["/settingsConfig/env/ANTHROPIC_AUTH_TOKEN"]
        );
        assert_eq!(b.status, ItemStatus::Identical);
        assert!(b.missing_secrets.is_empty());

        let mut secrets = BTreeMap::new();
        secrets.insert(
            a.key.clone(),
            BTreeMap::from([(
                "/settingsConfig/env/ANTHROPIC_AUTH_TOKEN".to_string(),
                "sk-typed".to_string(),
            )]),
        );
        let result = apply(
            &target,
            &bundle,
            &ApplyRequest {
                keys: p.items.iter().map(|i| i.key.clone()).collect(),
                overwrite: true,
                secrets,
            },
        );
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.added, 1);
        assert_eq!(result.skipped, 1);
        let a = target
            .db
            .get_provider_by_id("a", "claude")
            .unwrap()
            .unwrap();
        assert_eq!(a.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-typed");
        let b = target
            .db
            .get_provider_by_id("b", "claude")
            .unwrap()
            .unwrap();
        assert_eq!(b.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-local");
    }

    #[test]
    fn conflicts_are_skipped_unless_overwrite_and_keep_local_keys() {
        let _home = crate::test_support::isolated_home();
        let target = state();
        target
            .db
            .save_provider("claude", &claude("other", "k"))
            .unwrap();
        target.db.set_current_provider("claude", "other").unwrap();
        target
            .db
            .save_provider("claude", &claude("b", "sk-local"))
            .unwrap();
        let mut incoming = claude("b", "");
        incoming.settings_config["env"]["ANTHROPIC_BASE_URL"] = json!("https://new.example.com");
        let bundle = ShareBundle {
            format: FORMAT.into(),
            version: VERSION,
            created_at: String::new(),
            secrets_stripped: true,
            providers: vec![BundleProvider {
                app: "claude".into(),
                provider: incoming,
            }],
            mcp_servers: vec![],
            prompts: vec![],
        };
        let p = preview(&target, &bundle).unwrap();
        assert_eq!(p.items[0].status, ItemStatus::Conflict);
        assert!(p.items[0].missing_secrets.is_empty());
        let keys = vec![p.items[0].key.clone()];
        let skipped = apply(
            &target,
            &bundle,
            &ApplyRequest {
                keys: keys.clone(),
                overwrite: false,
                secrets: BTreeMap::new(),
            },
        );
        assert_eq!(skipped.skipped, 1);
        let updated = apply(
            &target,
            &bundle,
            &ApplyRequest {
                keys,
                overwrite: true,
                secrets: BTreeMap::new(),
            },
        );
        assert_eq!(updated.updated, 1, "{:?}", updated.errors);
        let b = target
            .db
            .get_provider_by_id("b", "claude")
            .unwrap()
            .unwrap();
        assert_eq!(
            b.settings_config["env"]["ANTHROPIC_BASE_URL"],
            "https://new.example.com"
        );
        assert_eq!(b.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-local");
    }
}
