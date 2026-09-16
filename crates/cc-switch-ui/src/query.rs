//! Minimal replacement for react-query: keyed async resources with explicit
//! invalidation. A query is a `use_resource` that also reads a per-key
//! version counter, so bumping the counter (from a mutation or a backend
//! event) re-runs every query sharing the key.

use std::collections::HashMap;
use std::future::Future;

use cc_switch_contract::IpcError;
use dioxus::prelude::*;

#[derive(Clone, Copy, Default)]
pub struct QueryClient {
    versions: Signal<HashMap<String, Signal<u64>>>,
}

impl QueryClient {
    fn version_signal(&self, key: &str) -> Signal<u64> {
        if let Some(existing) = self.versions.peek().get(key) {
            return *existing;
        }
        let created = Signal::new_in_scope(0, ScopeId::ROOT);
        let mut versions = self.versions;
        versions.write().insert(key.to_string(), created);
        created
    }

    /// Re-runs every query registered under `key`.
    pub fn invalidate(&self, key: &str) {
        let mut version = self.version_signal(key);
        version += 1;
    }

    /// Re-runs every query whose key starts with `prefix`
    /// (e.g. `"providers:"` for all apps).
    pub fn invalidate_prefix(&self, prefix: &str) {
        let signals: Vec<Signal<u64>> = self
            .versions
            .peek()
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(_, s)| *s)
            .collect();
        for mut signal in signals {
            signal += 1;
        }
    }

    pub fn invalidate_all(&self) {
        self.invalidate_prefix("");
    }
}

pub fn use_query_client_provider() -> QueryClient {
    use_context_provider(QueryClient::default)
}

pub fn use_query_client() -> QueryClient {
    use_context::<QueryClient>()
}

pub type QueryResult<T> = Resource<Result<T, IpcError>>;

/// Runs `fetch` now and again whenever the key returned by `key` is
/// invalidated or any signal read by `key`/`fetch` changes (so a key built
/// from a reactive prop follows that prop).
pub fn use_query<T, K, F, Fut>(key: K, fetch: F) -> QueryResult<T>
where
    T: 'static,
    K: Fn() -> String + 'static,
    F: Fn() -> Fut + 'static,
    Fut: Future<Output = Result<T, IpcError>> + 'static,
{
    let client = use_query_client();
    use_resource(move || {
        // Subscribe to invalidation of the current key.
        let version = client.version_signal(&key());
        let _ = version();
        fetch()
    })
}

/// Well-known query keys.
pub mod keys {
    use cc_switch_contract::AppId;

    pub fn providers(app: AppId) -> String {
        format!("providers:{app}")
    }

    pub fn current_provider(app: AppId) -> String {
        format!("current-provider:{app}")
    }

    pub const SETTINGS: &str = "settings";
    pub const PROXY_STATUS: &str = "proxy-status";
    pub const PROFILES: &str = "profiles";
    pub const USAGE: &str = "usage";
}
