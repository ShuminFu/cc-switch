//! 诊断报告：把排障需要的状态汇总成一份 Markdown（无密钥），用户可复制到
//! issue 里或保存成文件。只读，不改任何状态。

use crate::app_config::AppType;
use crate::database::{ProxyEvent, SwitchRule, SwitchRuleState};
use crate::proxy::types::{AppProxyConfig, ProxyStatus, ProxyTakeoverStatus};
use crate::store::AppState;
use std::fmt::Write as _;
use tauri::Manager;

/// 时间线取最近多少条
pub const RECENT_EVENTS: u32 = 30;

const PROXY_APPS: [&str; 4] = ["claude", "codex", "gemini", "grokbuild"];

#[derive(Debug, Clone)]
pub struct ProviderSummary {
    pub app_type: String,
    pub total: usize,
    pub current_name: Option<String>,
    pub current_category: Option<String>,
    pub in_failover_queue: usize,
}

#[derive(Debug, Clone, Default)]
pub struct DiagnosticsInput {
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub generated_at: String,
    pub schema_version: i32,
    pub settings: serde_json::Value,
    pub proxy_running: bool,
    pub proxy_status: Option<ProxyStatus>,
    pub takeover: Option<ProxyTakeoverStatus>,
    pub app_proxy_configs: Vec<AppProxyConfig>,
    pub providers: Vec<ProviderSummary>,
    pub switch_rules: Vec<SwitchRule>,
    pub switch_rule_states: Vec<SwitchRuleState>,
    pub events: Vec<ProxyEvent>,
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn format_ts(unix_seconds: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(unix_seconds, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| unix_seconds.to_string())
}

/// 把输入渲染成 Markdown。纯函数，便于测试。
pub fn render_report(input: &DiagnosticsInput) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# CC Switch diagnostics\n");
    let _ = writeln!(out, "- Version: {}", input.app_version);
    let _ = writeln!(out, "- Platform: {} / {}", input.os, input.arch);
    let _ = writeln!(out, "- Generated: {}", input.generated_at);
    let _ = writeln!(out, "- Database schema: v{}", input.schema_version);

    let _ = writeln!(out, "\n## Settings (device-level, no secrets)\n");
    if let Some(map) = input.settings.as_object() {
        for (key, value) in map {
            let _ = writeln!(out, "- {key}: {value}");
        }
    }

    let _ = writeln!(out, "\n## Local proxy\n");
    let _ = writeln!(out, "- Running: {}", yes_no(input.proxy_running));
    if let Some(status) = &input.proxy_status {
        let _ = writeln!(out, "- Listen: {}:{}", status.address, status.port);
        let _ = writeln!(
            out,
            "- Requests: {} total, {} ok, {} failed ({:.1}% success), {} active",
            status.total_requests,
            status.success_requests,
            status.failed_requests,
            status.success_rate,
            status.active_connections
        );
        let _ = writeln!(out, "- Uptime: {}s", status.uptime_seconds);
        let _ = writeln!(out, "- Failover count: {}", status.failover_count);
        if let Some(err) = &status.last_error {
            let _ = writeln!(out, "- Last error: {err}");
        }
    }
    if let Some(takeover) = &input.takeover {
        let _ = writeln!(
            out,
            "- Takeover: claude={} codex={} gemini={} grokbuild={} opencode={} openclaw={}",
            yes_no(takeover.claude),
            yes_no(takeover.codex),
            yes_no(takeover.gemini),
            yes_no(takeover.grokbuild),
            yes_no(takeover.opencode),
            yes_no(takeover.openclaw)
        );
    }
    if !input.app_proxy_configs.is_empty() {
        let _ = writeln!(
            out,
            "\n| App | Takeover | Auto failover | Retries | Breaker (fail/success/timeout/err rate/min) | Timeouts (first byte/idle/non-stream) |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|");
        for cfg in &input.app_proxy_configs {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {}/{}/{}s/{:.2}/{} | {}s/{}s/{}s |",
                cfg.app_type,
                yes_no(cfg.enabled),
                yes_no(cfg.auto_failover_enabled),
                cfg.max_retries,
                cfg.circuit_failure_threshold,
                cfg.circuit_success_threshold,
                cfg.circuit_timeout_seconds,
                cfg.circuit_error_rate_threshold,
                cfg.circuit_min_requests,
                cfg.streaming_first_byte_timeout,
                cfg.streaming_idle_timeout,
                cfg.non_streaming_timeout
            );
        }
    }

    let _ = writeln!(out, "\n## Providers\n");
    let _ = writeln!(
        out,
        "| App | Providers | Current | Category | In failover queue |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|");
    for p in &input.providers {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            p.app_type,
            p.total,
            p.current_name.as_deref().unwrap_or("-"),
            p.current_category.as_deref().unwrap_or("-"),
            p.in_failover_queue
        );
    }

    let _ = writeln!(out, "\n## Quota rules\n");
    if input.switch_rules.is_empty() {
        let _ = writeln!(out, "- none");
    }
    for rule in &input.switch_rules {
        let armed = input
            .switch_rule_states
            .iter()
            .find(|s| s.rule_id == rule.id);
        let _ = writeln!(
            out,
            "- [{}] {} / {} {} ≥ {:.0}% → {} (revert: {}){}",
            if rule.enabled { "on" } else { "off" },
            rule.app_type,
            rule.watched_provider_id.as_deref().unwrap_or("official"),
            rule.tier_name,
            rule.threshold_pct,
            rule.target_provider_id,
            yes_no(rule.revert_on_reset),
            armed
                .map(|s| format!(
                    " — armed since {} ({}), reverts {}",
                    format_ts(s.fired_at),
                    s.reason.as_deref().unwrap_or("-"),
                    s.resets_at
                        .map(format_ts)
                        .unwrap_or_else(|| "fallback".to_string())
                ))
                .unwrap_or_default()
        );
    }

    let _ = writeln!(out, "\n## Recent proxy events (newest first)\n");
    if input.events.is_empty() {
        let _ = writeln!(out, "- none");
    }
    for event in &input.events {
        let _ = writeln!(
            out,
            "- {} [{}] {}{}{}",
            format_ts(event.created_at),
            event.app_type,
            event.kind,
            event
                .provider_name
                .as_deref()
                .or(event.provider_id.as_deref())
                .map(|p| format!(" · {p}"))
                .unwrap_or_default(),
            event
                .detail
                .as_deref()
                .map(|d| format!(" · {d}"))
                .unwrap_or_default()
        );
    }
    out
}

/// 只挑设备级、非敏感的字段
fn settings_summary() -> serde_json::Value {
    let s = crate::settings::get_settings();
    serde_json::json!({
        "language": s.language,
        "showInTray": s.show_in_tray,
        "minimizeToTrayOnClose": s.minimize_to_tray_on_close,
        "launchOnStartup": s.launch_on_startup,
        "silentStartup": s.silent_startup,
        "enableLocalProxy": s.enable_local_proxy,
        "enableFailoverToggle": s.enable_failover_toggle,
        "switchRulesEnabled": s.switch_rules_enabled,
        "showProfileSwitcher": s.show_profile_switcher,
        "preserveCodexOfficialAuthOnSwitch": s.preserve_codex_official_auth_on_switch,
        "unifyCodexSessionHistory": s.unify_codex_session_history,
        "enableClaudePluginIntegration": s.enable_claude_plugin_integration,
        "visibleApps": s.visible_apps,
    })
}

/// 从运行时状态收集输入并渲染
pub async fn build_report(app: &tauri::AppHandle, state: &AppState) -> Result<String, String> {
    let proxy_running = state.proxy_service.is_running().await;
    let proxy_status = state.proxy_service.get_status().await.ok();
    let takeover = state.proxy_service.get_takeover_status().await.ok();

    let mut app_proxy_configs = Vec::new();
    for app_type in PROXY_APPS {
        if let Ok(cfg) = state.db.get_proxy_config_for_app(app_type).await {
            app_proxy_configs.push(cfg);
        }
    }

    let mut providers = Vec::new();
    for app_type in [
        AppType::Claude,
        AppType::ClaudeDesktop,
        AppType::Codex,
        AppType::Gemini,
        AppType::GrokBuild,
        AppType::OpenCode,
        AppType::OpenClaw,
        AppType::Hermes,
    ] {
        let all = state
            .db
            .get_all_providers(app_type.as_str())
            .map_err(|e| e.to_string())?;
        let current_id = crate::settings::get_effective_current_provider(&state.db, &app_type)
            .ok()
            .flatten();
        let current = current_id.as_ref().and_then(|id| all.get(id));
        providers.push(ProviderSummary {
            app_type: app_type.as_str().to_string(),
            total: all.len(),
            current_name: current.map(|p| p.name.clone()),
            current_category: current.and_then(|p| p.category.clone()),
            in_failover_queue: all.values().filter(|p| p.in_failover_queue).count(),
        });
    }

    let input = DiagnosticsInput {
        app_version: app.package_info().version.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        schema_version: crate::database::SCHEMA_VERSION,
        settings: settings_summary(),
        proxy_running,
        proxy_status,
        takeover,
        app_proxy_configs,
        providers,
        switch_rules: state
            .db
            .list_switch_rules(None)
            .map_err(|e| e.to_string())?,
        switch_rule_states: state
            .db
            .list_switch_rule_states()
            .map_err(|e| e.to_string())?,
        events: state
            .db
            .list_proxy_events(None, RECENT_EVENTS)
            .map_err(|e| e.to_string())?,
    };
    Ok(render_report(&input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_every_section_without_panicking_on_empty_input() {
        let report = render_report(&DiagnosticsInput {
            app_version: "3.17.0".to_string(),
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            generated_at: "2026-10-09T00:00:00+00:00".to_string(),
            schema_version: 17,
            settings: serde_json::json!({ "enableLocalProxy": true }),
            ..Default::default()
        });
        assert!(report.starts_with("# CC Switch diagnostics"));
        assert!(report.contains("- Version: 3.17.0"));
        assert!(report.contains("- Database schema: v17"));
        assert!(report.contains("- enableLocalProxy: true"));
        assert!(report.contains("## Local proxy"));
        assert!(report.contains("- Running: no"));
        assert!(report.contains("## Quota rules\n\n- none"));
        assert!(report.contains("## Recent proxy events (newest first)\n\n- none"));
    }

    #[test]
    fn renders_rules_events_and_providers() {
        let input = DiagnosticsInput {
            providers: vec![ProviderSummary {
                app_type: "claude".to_string(),
                total: 3,
                current_name: Some("My Relay".to_string()),
                current_category: Some("third_party".to_string()),
                in_failover_queue: 2,
            }],
            switch_rules: vec![SwitchRule {
                id: "r1".to_string(),
                app_type: "claude".to_string(),
                watched_provider_id: None,
                source: "subscription".to_string(),
                tier_name: "five_hour".to_string(),
                threshold_pct: 90.0,
                target_provider_id: "backup".to_string(),
                revert_on_reset: true,
                enabled: true,
                sort_index: 0,
                last_fired_at: None,
                created_at: 1,
            }],
            switch_rule_states: vec![SwitchRuleState {
                rule_id: "r1".to_string(),
                fired_at: 1_800_000_000,
                resets_at: Some(1_800_010_000),
                switched_from: "official".to_string(),
                switched_to: "backup".to_string(),
                last_utilization: 92.0,
                reason: Some("five_hour 92%".to_string()),
            }],
            events: vec![ProxyEvent {
                id: 1,
                created_at: 1_800_000_000,
                app_type: "claude".to_string(),
                kind: "failover_switch".to_string(),
                provider_id: Some("p2".to_string()),
                provider_name: Some("Backup".to_string()),
                detail: None,
            }],
            ..Default::default()
        };
        let report = render_report(&input);
        assert!(report.contains("| claude | 3 | My Relay | third_party | 2 |"));
        assert!(report.contains("[on] claude / official five_hour ≥ 90% → backup (revert: yes)"));
        assert!(report.contains("armed since 2027-01-15"));
        assert!(report.contains("five_hour 92%"));
        assert!(report.contains("[claude] failover_switch · Backup"));
    }
}
