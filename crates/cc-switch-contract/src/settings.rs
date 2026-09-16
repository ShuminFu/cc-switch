//! Application settings as serialized by `save_settings` / `get_settings`
//! (`src-tauri/src/settings.rs`, `#[serde(rename_all = "camelCase")]`).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibleApps {
    #[serde(default = "default_true")]
    pub claude: bool,
    #[serde(
        rename = "claude-desktop",
        alias = "claudeDesktop",
        alias = "claude_desktop",
        default = "default_true"
    )]
    pub claude_desktop: bool,
    #[serde(default = "default_true")]
    pub codex: bool,
    #[serde(default = "default_true")]
    pub gemini: bool,
    #[serde(default = "default_true")]
    pub grokbuild: bool,
    #[serde(default = "default_true")]
    pub opencode: bool,
    #[serde(default = "default_true")]
    pub openclaw: bool,
    #[serde(default)]
    pub hermes: bool,
}

impl Default for VisibleApps {
    fn default() -> Self {
        Self {
            claude: true,
            claude_desktop: true,
            codex: true,
            gemini: true,
            grokbuild: true,
            opencode: true,
            openclaw: true,
            hermes: false,
        }
    }
}

impl VisibleApps {
    pub fn is_visible(&self, app: crate::AppId) -> bool {
        use crate::AppId::*;
        match app {
            Claude => self.claude,
            ClaudeDesktop => self.claude_desktop,
            Codex => self.codex,
            Gemini => self.gemini,
            GrokBuild => self.grokbuild,
            OpenCode => self.opencode,
            OpenClaw => self.openclaw,
            Hermes => self.hermes,
        }
    }
}

/// The settings fields the shell needs. Everything else (sync settings,
/// directories, migrations, ...) rides along untouched in `extra` until the
/// view that edits it is ported and types the field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub show_in_tray: bool,
    #[serde(default = "default_true")]
    pub minimize_to_tray_on_close: bool,
    #[serde(default)]
    pub use_app_window_controls: bool,
    #[serde(default)]
    pub enable_claude_plugin_integration: bool,
    #[serde(default)]
    pub skip_claude_onboarding: bool,
    #[serde(default)]
    pub launch_on_startup: bool,
    #[serde(default)]
    pub silent_startup: bool,
    #[serde(default)]
    pub enable_local_proxy: bool,
    #[serde(default)]
    pub enable_failover_toggle: bool,
    #[serde(default = "default_true")]
    pub show_profile_switcher: bool,
    #[serde(default)]
    pub preserve_codex_official_auth_on_switch: bool,
    #[serde(default)]
    pub unify_codex_session_history: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_run_notice_confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_apps: Option<VisibleApps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_dashboard_refresh_interval_ms: Option<u32>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for AppSettings {
    fn default() -> Self {
        serde_json::from_value(Value::Object(Map::new())).expect("all fields have defaults")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_backend_defaults() {
        let s = AppSettings::default();
        assert!(s.show_in_tray);
        assert!(s.minimize_to_tray_on_close);
        assert!(s.show_profile_switcher);
        assert!(!s.enable_local_proxy);
        assert!(s.language.is_none());
    }

    #[test]
    fn preserves_untyped_fields_on_round_trip() {
        let raw = r#"{"showInTray":false,"language":"en","visibleApps":{"claude":true,"claude-desktop":false,"hermes":true},
            "webdavSync":{"enabled":true,"url":"https://dav"},"currentProviderClaude":"p1"}"#;
        let s: AppSettings = serde_json::from_str(raw).unwrap();
        assert!(!s.show_in_tray);
        let apps = s.visible_apps.as_ref().unwrap();
        assert!(!apps.claude_desktop);
        assert!(apps.hermes);
        assert!(!apps.is_visible(crate::AppId::ClaudeDesktop));
        assert_eq!(s.extra["webdavSync"]["url"], "https://dav");
        let out: Value = serde_json::to_value(&s).unwrap();
        assert_eq!(out["currentProviderClaude"], "p1");
        assert_eq!(out["visibleApps"]["claude-desktop"], false);
        assert!(out.get("firstRunNoticeConfirmed").is_none());
    }

    #[test]
    fn accepts_legacy_visible_apps_aliases() {
        let apps: VisibleApps = serde_json::from_str(r#"{"claudeDesktop":false}"#).unwrap();
        assert!(!apps.claude_desktop);
    }
}
