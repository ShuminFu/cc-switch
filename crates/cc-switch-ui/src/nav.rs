//! Top-level navigation: which tool is selected and which view is shown.
//! Storage keys and identifiers match the React app so switching UIs keeps
//! the user's place.

use cc_switch_contract::AppId;

use crate::storage;

pub const APP_STORAGE_KEY: &str = "cc-switch-last-app";
pub const VIEW_STORAGE_KEY: &str = "cc-switch-last-view";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum View {
    #[default]
    Providers,
    Settings,
    Prompts,
    Skills,
    SkillsDiscovery,
    Mcp,
    Agents,
    Universal,
    Sessions,
    Workspace,
    OpenClawEnv,
    OpenClawTools,
    OpenClawAgents,
    HermesMemory,
}

impl View {
    pub const ALL: [View; 14] = [
        View::Providers,
        View::Settings,
        View::Prompts,
        View::Skills,
        View::SkillsDiscovery,
        View::Mcp,
        View::Agents,
        View::Universal,
        View::Sessions,
        View::Workspace,
        View::OpenClawEnv,
        View::OpenClawTools,
        View::OpenClawAgents,
        View::HermesMemory,
    ];

    /// Identifier used by the React app (`localStorage["cc-switch-last-view"]`).
    pub fn as_str(self) -> &'static str {
        match self {
            View::Providers => "providers",
            View::Settings => "settings",
            View::Prompts => "prompts",
            View::Skills => "skills",
            View::SkillsDiscovery => "skillsDiscovery",
            View::Mcp => "mcp",
            View::Agents => "agents",
            View::Universal => "universal",
            View::Sessions => "sessions",
            View::Workspace => "workspace",
            View::OpenClawEnv => "openclawEnv",
            View::OpenClawTools => "openclawTools",
            View::OpenClawAgents => "openclawAgents",
            View::HermesMemory => "hermesMemory",
        }
    }

    pub fn from_str(s: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// Translation key of the header title; `None` for the providers view,
    /// whose header shows the app switcher instead.
    pub fn title_key(self) -> Option<&'static str> {
        Some(match self {
            View::Providers => return None,
            View::Settings => "settings.title",
            View::Prompts => "prompts.title",
            View::Skills | View::SkillsDiscovery => "skills.title",
            View::Mcp => "mcp.unifiedPanel.title",
            View::Agents => "agents.title",
            View::Universal => "universalProvider.title",
            View::Sessions => "sessionManager.title",
            View::Workspace => "workspace.title",
            View::OpenClawEnv => "openclaw.env.title",
            View::OpenClawTools => "openclaw.tools.title",
            View::OpenClawAgents => "openclaw.agents.title",
            View::HermesMemory => "hermes.memory.title",
        })
    }

    /// Where the header back button leads.
    pub fn back_target(self) -> View {
        match self {
            View::SkillsDiscovery => View::Skills,
            _ => View::Providers,
        }
    }

    /// Views whose header shows the app switcher.
    pub fn shows_app_switcher(self) -> bool {
        matches!(
            self,
            View::Providers
                | View::Workspace
                | View::Sessions
                | View::OpenClawEnv
                | View::OpenClawTools
                | View::OpenClawAgents
        )
    }
}

pub fn initial_view() -> View {
    storage::get(VIEW_STORAGE_KEY)
        .and_then(|s| View::from_str(&s))
        .unwrap_or_default()
}

pub fn initial_app() -> AppId {
    storage::get(APP_STORAGE_KEY)
        .and_then(|s| s.parse().ok())
        .unwrap_or(AppId::Claude)
}

/// Display name of a tool, as shown in the app switcher.
pub fn app_display_name(app: AppId) -> &'static str {
    match app {
        AppId::Claude => "Claude Code",
        AppId::ClaudeDesktop => "Claude Desktop",
        AppId::Codex => "Codex",
        AppId::Gemini => "Gemini",
        AppId::GrokBuild => "Grok Build",
        AppId::OpenCode => "OpenCode",
        AppId::OpenClaw => "OpenClaw",
        AppId::Hermes => "Hermes",
    }
}

/// Icon key of a tool in the provider icon set.
pub fn app_icon_name(app: AppId) -> &'static str {
    match app {
        AppId::Claude | AppId::ClaudeDesktop => "claude",
        AppId::Codex => "openai",
        AppId::Gemini => "gemini",
        AppId::GrokBuild => "grok",
        AppId::OpenCode => "opencode",
        AppId::OpenClaw => "openclaw",
        AppId::Hermes => "hermes",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_identifiers_round_trip() {
        for view in View::ALL {
            assert_eq!(View::from_str(view.as_str()), Some(view));
        }
        assert_eq!(View::from_str("nope"), None);
        assert_eq!(View::SkillsDiscovery.back_target(), View::Skills);
        assert_eq!(View::Mcp.back_target(), View::Providers);
    }

    #[test]
    fn every_title_key_exists_in_the_locale_files() {
        for view in View::ALL {
            if let Some(key) = view.title_key() {
                assert!(
                    crate::i18n::has_key(crate::i18n::Locale::En, key),
                    "missing translation key {key} for {view:?}"
                );
            }
        }
    }
}
