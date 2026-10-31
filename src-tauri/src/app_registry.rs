//! One place to enumerate apps by capability (CMD-18).
//!
//! Command, startup, tray and CLI code iterate these helpers instead of
//! hand-written `["claude", "codex", ...]` lists, so a new [`AppType`] (and its
//! adapter in `adapters/`) is picked up everywhere. Capabilities come from the
//! adapter registry ([`crate::adapters::AdapterCapabilities`]).

use crate::adapters;
use crate::app_config::AppType;

/// Every app, in display order.
pub(crate) fn all_app_types() -> impl Iterator<Item = AppType> {
    AppType::all()
}

/// Apps whose live config the local proxy can take over (startup restore,
/// headless proxy, tray sections).
pub(crate) fn takeover_app_types() -> impl Iterator<Item = AppType> {
    adapters::takeover_adapters().map(|adapter| adapter.app())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takeover_apps_are_a_subset_of_all_apps() {
        let all: Vec<_> = all_app_types().collect();
        let takeover: Vec<_> = takeover_app_types().collect();
        assert!(!takeover.is_empty());
        for app in &takeover {
            assert!(all.contains(app));
            assert!(adapters::adapter_for(app).capabilities().proxy_takeover);
        }
    }

    /// The current takeover set; update together with the adapter capability.
    #[test]
    fn takeover_apps_match_the_known_proxy_routes() {
        let takeover: Vec<_> = takeover_app_types()
            .map(|a| a.as_str().to_string())
            .collect();
        assert_eq!(takeover, ["claude", "codex", "gemini", "grokbuild"]);
    }
}
