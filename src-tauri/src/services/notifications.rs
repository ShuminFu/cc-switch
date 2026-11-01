//! System notifications (PRODUCT-06).
//!
//! OS-level notifications for events a user in tray-only / Lightweight Mode
//! would otherwise miss: failover switches, a circuit breaker opening, a
//! provider budget being exceeded, a subscription quota running out and
//! background sync errors.
//!
//! Every category is individually toggleable under `settings.notifications`;
//! only failover and circuit-open are on by default. Each `(category, subject)`
//! pair is sent at most once per [`DEDUP_WINDOW`], so an upstream 500 storm or
//! a flapping breaker produces a single notification.
//!
//! Other modules call the `notify_*` helpers; they are cheap no-ops when the
//! category is disabled or was notified recently.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::plugin::{Builder as PluginBuilder, TauriPlugin};
use tauri::{AppHandle, Manager, Runtime};

use crate::proxy::circuit_breaker::{add_state_listener, CircuitState, CircuitStateChange};

/// At most one notification per category/subject in this window.
pub const DEDUP_WINDOW: Duration = Duration::from_secs(5 * 60);

/// Notification categories, each with its own toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationCategory {
    Failover,
    CircuitOpen,
    BudgetExceeded,
    QuotaExhausted,
    SyncError,
}

/// Per-category toggles, stored in `settings.json` under `notifications`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    pub failover: bool,
    pub circuit_open: bool,
    pub budget_exceeded: bool,
    pub quota_exhausted: bool,
    pub sync_error: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            failover: true,
            circuit_open: true,
            budget_exceeded: false,
            quota_exhausted: false,
            sync_error: false,
        }
    }
}

impl NotificationSettings {
    pub fn allows(&self, category: NotificationCategory) -> bool {
        match category {
            NotificationCategory::Failover => self.failover,
            NotificationCategory::CircuitOpen => self.circuit_open,
            NotificationCategory::BudgetExceeded => self.budget_exceeded,
            NotificationCategory::QuotaExhausted => self.quota_exhausted,
            NotificationCategory::SyncError => self.sync_error,
        }
    }

    /// Current settings from `settings.json`.
    pub fn current() -> Self {
        crate::settings::get_settings()
            .notifications
            .unwrap_or_default()
    }
}

/// Remembers when each `(category, subject)` was last notified.
#[derive(Debug)]
pub struct NotificationDeduper {
    window: Duration,
    last_sent: HashMap<(NotificationCategory, String), Instant>,
}

impl NotificationDeduper {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            last_sent: HashMap::new(),
        }
    }

    /// Returns `true` (and records the send) when no notification for this
    /// category/subject went out within the window.
    pub fn should_send(
        &mut self,
        category: NotificationCategory,
        subject: &str,
        now: Instant,
    ) -> bool {
        let window = self.window;
        self.last_sent
            .retain(|_, sent| now.saturating_duration_since(*sent) < window);
        let key = (category, subject.to_string());
        if self.last_sent.contains_key(&key) {
            return false;
        }
        self.last_sent.insert(key, now);
        true
    }
}

static DEDUPER: std::sync::LazyLock<Mutex<NotificationDeduper>> =
    std::sync::LazyLock::new(|| Mutex::new(NotificationDeduper::new(DEDUP_WINDOW)));

/// Decide whether a notification should be shown: the category must be
/// enabled and not notified for this subject within the de-dup window.
pub fn should_notify(
    settings: &NotificationSettings,
    deduper: &mut NotificationDeduper,
    category: NotificationCategory,
    subject: &str,
    now: Instant,
) -> bool {
    settings.allows(category) && deduper.should_send(category, subject, now)
}

/// Show an OS notification if `category` is enabled and not de-duplicated.
/// Returns whether a notification was handed to the OS.
pub fn notify<R: Runtime>(
    app: &AppHandle<R>,
    category: NotificationCategory,
    subject: &str,
    title: &str,
    body: &str,
) -> bool {
    let settings = NotificationSettings::current();
    let allowed = match DEDUPER.lock() {
        Ok(mut deduper) => {
            should_notify(&settings, &mut deduper, category, subject, Instant::now())
        }
        Err(_) => false,
    };
    if !allowed {
        return false;
    }
    use tauri_plugin_notification::NotificationExt;
    match app.notification().builder().title(title).body(body).show() {
        Ok(()) => true,
        Err(e) => {
            log::warn!("[Notify] failed to show {category:?} notification: {e}");
            false
        }
    }
}

// ===== Localised text =====

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lang {
    En,
    Zh,
    ZhTw,
    Ja,
}

fn current_lang() -> Lang {
    let settings = crate::settings::get_settings();
    let language = match settings.language.as_deref() {
        Some(lang) => lang.to_string(),
        None => crate::tray::detect_system_tray_language().to_string(),
    };
    lang_from(&language)
}

fn lang_from(language: &str) -> Lang {
    match language {
        "zh" => Lang::Zh,
        "zh-TW" => Lang::ZhTw,
        "ja" => Lang::Ja,
        _ => Lang::En,
    }
}

fn app_label(app_type: &str) -> String {
    match app_type {
        "claude" => "Claude Code".to_string(),
        "claude-desktop" | "claude_desktop" => "Claude Desktop".to_string(),
        "codex" => "Codex".to_string(),
        "gemini" => "Gemini CLI".to_string(),
        "grokbuild" => "Grok Build".to_string(),
        "opencode" => "OpenCode".to_string(),
        "openclaw" => "OpenClaw".to_string(),
        "hermes" => "Hermes".to_string(),
        other => other.to_string(),
    }
}

fn failover_text(lang: Lang, app: &str, provider: &str) -> (String, String) {
    match lang {
        Lang::Zh => (
            format!("{app} 已故障转移"),
            format!("当前供应商出现故障，已自动切换到 {provider}"),
        ),
        Lang::ZhTw => (
            format!("{app} 已故障轉移"),
            format!("目前供應商發生故障，已自動切換到 {provider}"),
        ),
        Lang::Ja => (
            format!("{app} がフェイルオーバーしました"),
            format!("現在のプロバイダーで障害が発生したため、{provider} に自動で切り替えました"),
        ),
        Lang::En => (
            format!("{app} failed over"),
            format!("The active provider failed; switched to {provider}"),
        ),
    }
}

fn circuit_text(lang: Lang, app: &str, provider: &str) -> (String, String) {
    match lang {
        Lang::Zh => (
            format!("{app}：{provider} 已熔断"),
            "连续请求失败，暂时停止向该供应商发送请求".to_string(),
        ),
        Lang::ZhTw => (
            format!("{app}：{provider} 已熔斷"),
            "連續請求失敗，暫時停止向該供應商發送請求".to_string(),
        ),
        Lang::Ja => (
            format!("{app}：{provider} のサーキットが開きました"),
            "リクエストの失敗が続いたため、このプロバイダーへの送信を一時停止しています"
                .to_string(),
        ),
        Lang::En => (
            format!("{app}: circuit opened for {provider}"),
            "Requests kept failing; this provider is paused for now".to_string(),
        ),
    }
}

fn budget_text(
    lang: Lang,
    provider: &str,
    period: BudgetPeriod,
    spent: f64,
    limit: f64,
) -> (String, String) {
    let daily = period == BudgetPeriod::Daily;
    match lang {
        Lang::Zh => (
            format!(
                "{provider} 已超出{}预算",
                if daily { "每日" } else { "每月" }
            ),
            format!("已用 ${spent:.2}，上限 ${limit:.2}"),
        ),
        Lang::ZhTw => (
            format!(
                "{provider} 已超出{}預算",
                if daily { "每日" } else { "每月" }
            ),
            format!("已用 ${spent:.2}，上限 ${limit:.2}"),
        ),
        Lang::Ja => (
            format!(
                "{provider} が{}予算を超えました",
                if daily { "1日の" } else { "月間" }
            ),
            format!("使用額 ${spent:.2} / 上限 ${limit:.2}"),
        ),
        Lang::En => (
            format!(
                "{provider} exceeded its {} budget",
                if daily { "daily" } else { "monthly" }
            ),
            format!("Spent ${spent:.2} of ${limit:.2}"),
        ),
    }
}

fn quota_text(lang: Lang, app: &str, tier: &str) -> (String, String) {
    match lang {
        Lang::Zh => (
            format!("{app} 订阅额度已用尽"),
            format!("额度窗口 {tier} 已达 100%"),
        ),
        Lang::ZhTw => (
            format!("{app} 訂閱額度已用盡"),
            format!("額度視窗 {tier} 已達 100%"),
        ),
        Lang::Ja => (
            format!("{app} のサブスクリプション枠を使い切りました"),
            format!("{tier} の枠が 100% に達しました"),
        ),
        Lang::En => (
            format!("{app} subscription quota exhausted"),
            format!("The {tier} window is at 100%"),
        ),
    }
}

fn sync_text(lang: Lang, backend: &str, error: &str) -> (String, String) {
    let error = truncate(error, 200);
    match lang {
        Lang::Zh => (format!("{backend} 自动同步失败"), error),
        Lang::ZhTw => (format!("{backend} 自動同步失敗"), error),
        Lang::Ja => (format!("{backend} の自動同期に失敗しました"), error),
        Lang::En => (format!("{backend} auto sync failed"), error),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}

// ===== Public helpers for other modules =====

/// Budget window for [`notify_budget_exceeded`].
#[allow(dead_code)] // constructed by the budget enforcement code (PRODUCT-05)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetPeriod {
    Daily,
    Monthly,
}

/// Failover switched the active provider of `app_type` to `provider_name`.
pub fn notify_failover_switched<R: Runtime>(
    app: &AppHandle<R>,
    app_type: &str,
    provider_id: &str,
    provider_name: &str,
) -> bool {
    let (title, body) = failover_text(current_lang(), &app_label(app_type), provider_name);
    notify(
        app,
        NotificationCategory::Failover,
        &format!("{app_type}:{provider_id}"),
        &title,
        &body,
    )
}

/// A provider's circuit breaker opened.
pub fn notify_circuit_opened<R: Runtime>(
    app: &AppHandle<R>,
    app_type: &str,
    provider_id: &str,
    provider_name: &str,
) -> bool {
    let (title, body) = circuit_text(current_lang(), &app_label(app_type), provider_name);
    notify(
        app,
        NotificationCategory::CircuitOpen,
        &format!("{app_type}:{provider_id}"),
        &title,
        &body,
    )
}

/// A provider exceeded its daily/monthly USD limit. Intended for the budget
/// enforcement code (PRODUCT-05) to call when it detects a breach.
#[allow(dead_code)] // called by the budget enforcement code (PRODUCT-05)
pub fn notify_budget_exceeded<R: Runtime>(
    app: &AppHandle<R>,
    app_type: &str,
    provider_id: &str,
    provider_name: &str,
    period: BudgetPeriod,
    spent_usd: f64,
    limit_usd: f64,
) -> bool {
    let (title, body) = budget_text(current_lang(), provider_name, period, spent_usd, limit_usd);
    let period_key = match period {
        BudgetPeriod::Daily => "daily",
        BudgetPeriod::Monthly => "monthly",
    };
    notify(
        app,
        NotificationCategory::BudgetExceeded,
        &format!("{app_type}:{provider_id}:{period_key}"),
        &title,
        &body,
    )
}

/// A subscription quota window (`tier`, e.g. `five_hour`) reached 100%.
pub fn notify_quota_exhausted<R: Runtime>(app: &AppHandle<R>, app_type: &str, tier: &str) -> bool {
    let (title, body) = quota_text(current_lang(), &app_label(app_type), tier);
    notify(
        app,
        NotificationCategory::QuotaExhausted,
        &format!("{app_type}:{tier}"),
        &title,
        &body,
    )
}

/// Names of the quota tiers that are exhausted (utilization >= 100%).
pub fn exhausted_tiers(quota: &crate::services::subscription::SubscriptionQuota) -> Vec<&str> {
    if !quota.success {
        return Vec::new();
    }
    quota
        .tiers
        .iter()
        .filter(|tier| tier.utilization >= 100.0)
        .map(|tier| tier.name.as_str())
        .collect()
}

/// Notify for every exhausted tier of a freshly queried subscription quota.
pub fn notify_quota_if_exhausted<R: Runtime>(
    app: &AppHandle<R>,
    app_type: &str,
    quota: &crate::services::subscription::SubscriptionQuota,
) {
    for tier in exhausted_tiers(quota) {
        notify_quota_exhausted(app, app_type, tier);
    }
}

/// A background (auto) sync to `backend` ("WebDAV", "S3") failed.
pub fn notify_sync_error<R: Runtime>(app: &AppHandle<R>, backend: &str, error: &str) -> bool {
    let (title, body) = sync_text(current_lang(), backend, error);
    notify(app, NotificationCategory::SyncError, backend, &title, &body)
}

/// Breaker transitions that warrant a notification: only a fresh opening
/// (Closed → Open). A failed HalfOpen probe (HalfOpen → Open) means "still
/// down" and is not news.
fn is_fresh_open(change: &CircuitStateChange) -> bool {
    change.from == CircuitState::Closed && change.to == CircuitState::Open
}

fn on_circuit_change(app: &AppHandle, change: &CircuitStateChange) {
    if !is_fresh_open(change) {
        return;
    }
    let Some((app_type, provider_id)) = change.app_and_provider() else {
        return;
    };
    let name = app
        .try_state::<crate::store::AppState>()
        .and_then(|state| {
            state
                .db
                .get_provider_by_id(provider_id, app_type)
                .ok()
                .flatten()
        })
        .map(|p| p.name)
        .unwrap_or_else(|| provider_id.to_string());
    notify_circuit_opened(app, app_type, provider_id, &name);
}

/// Tauri plugin that registers `tauri-plugin-notification`, subscribes to
/// circuit breaker transitions and starts the background health prober.
pub fn plugin() -> TauriPlugin<tauri::Wry> {
    PluginBuilder::new("cc-switch-alerts")
        .setup(|app, _api| {
            app.plugin(tauri_plugin_notification::init())?;
            let handle = app.clone();
            add_state_listener(std::sync::Arc::new(move |change: &CircuitStateChange| {
                on_circuit_change(&handle, change);
            }));
            crate::proxy::health::spawn(app.clone());
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::subscription::{QuotaTier, SubscriptionQuota};
    use serde_json::json;

    #[test]
    fn defaults_enable_only_failover_and_circuit() {
        let s = NotificationSettings::default();
        assert!(s.allows(NotificationCategory::Failover));
        assert!(s.allows(NotificationCategory::CircuitOpen));
        assert!(!s.allows(NotificationCategory::BudgetExceeded));
        assert!(!s.allows(NotificationCategory::QuotaExhausted));
        assert!(!s.allows(NotificationCategory::SyncError));

        // Missing fields / missing object fall back to the same defaults.
        let parsed: NotificationSettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(parsed, s);
        let parsed: NotificationSettings =
            serde_json::from_value(json!({ "syncError": true, "failover": false })).unwrap();
        assert!(parsed.sync_error);
        assert!(!parsed.failover);
        assert!(parsed.circuit_open);
        assert_eq!(
            crate::settings::AppSettings::default()
                .notifications
                .unwrap_or_default(),
            s
        );
    }

    #[test]
    fn settings_round_trip_in_camel_case() {
        let value = serde_json::to_value(NotificationSettings::default()).unwrap();
        assert_eq!(
            value,
            json!({
                "failover": true,
                "circuitOpen": true,
                "budgetExceeded": false,
                "quotaExhausted": false,
                "syncError": false,
            })
        );
    }

    #[test]
    fn deduper_allows_one_per_category_and_subject_per_window() {
        let mut d = NotificationDeduper::new(DEDUP_WINDOW);
        let t0 = Instant::now();
        assert!(d.should_send(NotificationCategory::Failover, "claude:a", t0));
        assert!(!d.should_send(
            NotificationCategory::Failover,
            "claude:a",
            t0 + Duration::from_secs(60)
        ));
        // Different subject or category is independent.
        assert!(d.should_send(NotificationCategory::Failover, "claude:b", t0));
        assert!(d.should_send(NotificationCategory::CircuitOpen, "claude:a", t0));
        // After the window it may fire again.
        assert!(!d.should_send(
            NotificationCategory::Failover,
            "claude:a",
            t0 + DEDUP_WINDOW - Duration::from_secs(1)
        ));
        assert!(d.should_send(
            NotificationCategory::Failover,
            "claude:a",
            t0 + DEDUP_WINDOW
        ));
    }

    #[test]
    fn storm_produces_a_single_notification() {
        let settings = NotificationSettings::default();
        let mut d = NotificationDeduper::new(DEDUP_WINDOW);
        let t0 = Instant::now();
        let sent = (0..50)
            .filter(|i| {
                should_notify(
                    &settings,
                    &mut d,
                    NotificationCategory::Failover,
                    "claude:backup",
                    t0 + Duration::from_secs(*i),
                )
            })
            .count();
        assert_eq!(sent, 1);
    }

    #[test]
    fn disabled_category_never_notifies_or_consumes_dedup_slot() {
        let mut settings = NotificationSettings::default();
        let mut d = NotificationDeduper::new(DEDUP_WINDOW);
        let t0 = Instant::now();
        assert!(!should_notify(
            &settings,
            &mut d,
            NotificationCategory::SyncError,
            "WebDAV",
            t0
        ));
        settings.sync_error = true;
        assert!(should_notify(
            &settings,
            &mut d,
            NotificationCategory::SyncError,
            "WebDAV",
            t0
        ));
    }

    #[test]
    fn only_fresh_openings_notify() {
        let change = |from, to| CircuitStateChange {
            key: "claude:p".into(),
            from,
            to,
        };
        assert!(is_fresh_open(&change(
            CircuitState::Closed,
            CircuitState::Open
        )));
        assert!(!is_fresh_open(&change(
            CircuitState::HalfOpen,
            CircuitState::Open
        )));
        assert!(!is_fresh_open(&change(
            CircuitState::Open,
            CircuitState::HalfOpen
        )));
        assert!(!is_fresh_open(&change(
            CircuitState::HalfOpen,
            CircuitState::Closed
        )));
    }

    #[test]
    fn exhausted_tiers_only_reports_full_windows() {
        let tier = |name: &str, utilization: f64| QuotaTier {
            name: name.to_string(),
            utilization,
            resets_at: None,
            used_value_usd: None,
            max_value_usd: None,
        };
        let mut quota = SubscriptionQuota::not_found("claude");
        quota.success = true;
        quota.tiers = vec![tier("five_hour", 100.0), tier("seven_day", 42.0)];
        assert_eq!(exhausted_tiers(&quota), vec!["five_hour"]);
        quota.success = false;
        assert!(exhausted_tiers(&quota).is_empty());
    }

    #[test]
    fn texts_are_localised_and_sync_errors_truncated() {
        assert_eq!(lang_from("zh"), Lang::Zh);
        assert_eq!(lang_from("zh-TW"), Lang::ZhTw);
        assert_eq!(lang_from("ja"), Lang::Ja);
        assert_eq!(lang_from("en"), Lang::En);
        assert_eq!(lang_from("fr"), Lang::En);
        let (title, body) = failover_text(Lang::En, "Claude Code", "Backup");
        assert_eq!(title, "Claude Code failed over");
        assert!(body.contains("Backup"));
        let (_, body) = sync_text(Lang::En, "WebDAV", &"x".repeat(500));
        assert_eq!(body.chars().count(), 201);
        let (title, body) = budget_text(Lang::En, "Relay", BudgetPeriod::Daily, 1.5, 1.0);
        assert_eq!(title, "Relay exceeded its daily budget");
        assert_eq!(body, "Spent $1.50 of $1.00");
    }
}
