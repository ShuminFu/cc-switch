//! Application identifiers shared by settings, providers and navigation.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// The CLI/desktop tools cc-switch manages. Wire form matches the TS `AppId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppId {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "claude-desktop")]
    ClaudeDesktop,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "grokbuild")]
    GrokBuild,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "openclaw")]
    OpenClaw,
    #[serde(rename = "hermes")]
    Hermes,
}

impl AppId {
    pub const ALL: [AppId; 8] = [
        AppId::Claude,
        AppId::ClaudeDesktop,
        AppId::Codex,
        AppId::Gemini,
        AppId::GrokBuild,
        AppId::OpenCode,
        AppId::OpenClaw,
        AppId::Hermes,
    ];

    /// Wire identifier, identical to the TS `AppId` union values.
    pub fn as_str(self) -> &'static str {
        match self {
            AppId::Claude => "claude",
            AppId::ClaudeDesktop => "claude-desktop",
            AppId::Codex => "codex",
            AppId::Gemini => "gemini",
            AppId::GrokBuild => "grokbuild",
            AppId::OpenCode => "opencode",
            AppId::OpenClaw => "openclaw",
            AppId::Hermes => "hermes",
        }
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AppId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AppId::ALL
            .iter()
            .copied()
            .find(|a| a.as_str() == s)
            .ok_or_else(|| format!("unknown app id `{s}`"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names_round_trip() {
        for app in AppId::ALL {
            let json = serde_json::to_string(&app).unwrap();
            assert_eq!(json, format!("\"{}\"", app.as_str()));
            assert_eq!(serde_json::from_str::<AppId>(&json).unwrap(), app);
            assert_eq!(app.as_str().parse::<AppId>().unwrap(), app);
        }
        assert!("nope".parse::<AppId>().is_err());
    }
}
