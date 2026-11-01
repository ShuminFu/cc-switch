//! Provider routing beyond the fixed failover order (PROXY-19, first increment).
//!
//! Every feature here is opt-in per app and off by default, so an app without
//! a routing config behaves exactly as before (failover queue order, attempts
//! bounded by `max_retries`).
//!
//! - **Strategy** decides which available provider is tried first:
//!   `priority` (queue order, the default), `roundRobin` (rotate the start per
//!   request) or `weighted` (smooth weighted round-robin over `weights`, the
//!   nginx algorithm: deterministic, no RNG). The remaining providers keep
//!   their queue order as the failover chain.
//! - **Sticky sessions** pin a client-provided session id to the provider
//!   that last served it. While that provider is still selectable (its
//!   breaker allows it) it is tried first, so a conversation keeps its prompt
//!   cache and thinking-signature domain after an earlier provider recovers.
//!   The pin expires `stickyTtlSecs` after its last use. The map is bounded.
//! - **Model rules** (PRODUCT-13) route by requested model: the first rule
//!   whose glob `pattern` matches the request's `model` restricts the
//!   candidates to its `providers` (in that order; empty = every provider)
//!   and may override the strategy. When none of a rule's providers is
//!   available the request falls back to the full list unless the rule is
//!   `exclusive`. The rule that fired is reported for the request trace.
//! - **Least latency** orders providers by an exponentially weighted moving
//!   average of their recent successful attempt latency (providers without a
//!   sample yet are tried first, so each gets measured); **cheapest** orders
//!   them by the provider's `costMultiplier` (missing = 1).
//! - **Concurrency limit** caps in-flight requests per provider
//!   (`maxConcurrentPerProvider`, 0 = unlimited). A provider at its limit is
//!   skipped like an open breaker; the permit is held until the response
//!   (including a stream) ends.
//!
//! Configs are stored as one JSON object in the `settings` key/value table
//! under [`ROUTING_SETTINGS_KEY`] (`{ "<app>": RoutingConfig }`), so no schema
//! migration is needed. The request path reads them through a short-lived
//! cache ([`CONFIG_CACHE_TTL`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;

/// `settings` key holding every app's routing config as a JSON object.
pub const ROUTING_SETTINGS_KEY: &str = "proxy_routing_config";
/// How long the request path trusts a cached config before re-reading it.
pub const CONFIG_CACHE_TTL: Duration = Duration::from_secs(5);
/// Default sticky pin lifetime when `stickyTtlSecs` is 0.
pub const DEFAULT_STICKY_TTL_SECS: u64 = 30 * 60;
/// Upper bound on remembered session pins (oldest evicted first).
pub const MAX_STICKY_SESSIONS: usize = 4096;

/// Which available provider a request tries first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RoutingStrategy {
    /// Failover queue order (today's behaviour).
    #[default]
    Priority,
    /// Rotate the first provider per request.
    RoundRobin,
    /// Smooth weighted round-robin over [`RoutingConfig::weights`].
    Weighted,
    /// Lowest recent latency first (EWMA of successful attempts).
    LeastLatency,
    /// Lowest `costMultiplier` first.
    Cheapest,
}

/// Route requests for matching models to specific providers (PRODUCT-13).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelRoutingRule {
    /// Label shown in the request trace; defaults to the pattern.
    pub name: String,
    /// Glob on the requested model, `*` matches any run of characters
    /// (case-insensitive), e.g. `claude-haiku-*`.
    pub pattern: String,
    /// Candidate provider ids in preference order; empty = all providers.
    pub providers: Vec<String>,
    /// Strategy among the candidates; `None` = the app's strategy.
    pub strategy: Option<RoutingStrategy>,
    /// Never fall back to providers outside `providers`.
    pub exclusive: bool,
}

impl ModelRoutingRule {
    pub fn label(&self) -> &str {
        if self.name.trim().is_empty() {
            &self.pattern
        } else {
            &self.name
        }
    }

    pub fn matches(&self, model: &str) -> bool {
        glob_match(
            &self.pattern.to_ascii_lowercase(),
            &model.to_ascii_lowercase(),
        )
    }
}

/// `*`-only glob match (no `?` or classes), iterative with backtracking.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let (p, t) = (pattern.as_bytes(), text.as_bytes());
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// Result of routing one request.
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub providers: Vec<Provider>,
    /// Label of the model rule that fired.
    pub rule: Option<String>,
    /// Whether the rule's candidates were unavailable and the full list was used.
    pub rule_fell_back: bool,
    pub strategy: RoutingStrategy,
}

/// Per-app routing settings. Every default reproduces the fixed priority list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RoutingConfig {
    pub strategy: RoutingStrategy,
    pub sticky_sessions: bool,
    /// Sticky pin lifetime after its last use; 0 means [`DEFAULT_STICKY_TTL_SECS`].
    pub sticky_ttl_secs: u64,
    /// In-flight requests allowed per provider; 0 = unlimited.
    pub max_concurrent_per_provider: u32,
    /// Provider id -> weight for [`RoutingStrategy::Weighted`]; missing = 1, 0 = never first.
    pub weights: HashMap<String, u32>,
    /// Model-aware rules, first match wins (PRODUCT-13).
    pub model_rules: Vec<ModelRoutingRule>,
}

impl RoutingConfig {
    /// True when the config changes nothing about routing.
    pub fn is_default_behaviour(&self) -> bool {
        self.strategy == RoutingStrategy::Priority
            && !self.sticky_sessions
            && self.max_concurrent_per_provider == 0
            && self.model_rules.is_empty()
    }

    /// The first model rule matching `model`.
    pub fn rule_for(&self, model: &str) -> Option<&ModelRoutingRule> {
        self.model_rules.iter().find(|r| r.matches(model))
    }

    fn sticky_ttl(&self) -> Duration {
        let secs = if self.sticky_ttl_secs == 0 {
            DEFAULT_STICKY_TTL_SECS
        } else {
            self.sticky_ttl_secs
        };
        Duration::from_secs(secs)
    }

    fn weight_of(&self, provider_id: &str) -> i64 {
        i64::from(self.weights.get(provider_id).copied().unwrap_or(1))
    }
}

// ─── Persistence ─────────────────────────────────────────────

/// Every app's routing config (missing or unparsable value = empty map).
pub fn load_routing_configs(db: &Database) -> Result<HashMap<String, RoutingConfig>, AppError> {
    let Some(raw) = db.get_setting(ROUTING_SETTINGS_KEY)? else {
        return Ok(HashMap::new());
    };
    match serde_json::from_str(&raw) {
        Ok(map) => Ok(map),
        Err(e) => {
            log::warn!("[Routing] ignoring unparsable {ROUTING_SETTINGS_KEY}: {e}");
            Ok(HashMap::new())
        }
    }
}

/// The routing config for one app (default when unset).
pub fn load_routing_config(db: &Database, app_type: &str) -> Result<RoutingConfig, AppError> {
    Ok(load_routing_configs(db)?
        .remove(app_type)
        .unwrap_or_default())
}

/// Store one app's routing config, keeping the other apps' entries.
pub fn save_routing_config(
    db: &Database,
    app_type: &str,
    config: &RoutingConfig,
) -> Result<(), AppError> {
    let mut all = load_routing_configs(db)?;
    if config == &RoutingConfig::default() {
        all.remove(app_type);
    } else {
        all.insert(app_type.to_string(), config.clone());
    }
    let raw = serde_json::to_string(&all)
        .map_err(|e| AppError::Message(format!("serialize routing config: {e}")))?;
    db.set_setting(ROUTING_SETTINGS_KEY, &raw)
}

// ─── Runtime state ───────────────────────────────────────────

struct StickyEntry {
    provider_id: String,
    last_used: Instant,
}

struct LimitEntry {
    limit: u32,
    semaphore: Arc<Semaphore>,
}

/// Shared routing state (lives in the `ProviderRouter`, survives requests).
#[derive(Default)]
pub struct RoutingState {
    configs: RwLock<HashMap<String, (Arc<RoutingConfig>, Instant)>>,
    sticky: Mutex<HashMap<(String, String), StickyEntry>>,
    round_robin: Mutex<HashMap<String, usize>>,
    /// Smooth WRR current weights per app, keyed by provider id.
    weighted: Mutex<HashMap<String, HashMap<String, i64>>>,
    limits: Mutex<HashMap<String, LimitEntry>>,
    /// EWMA of successful attempt latency in ms, keyed by `app:provider`.
    latency: Mutex<HashMap<String, f64>>,
}

/// Weight of the newest sample in the latency EWMA.
const LATENCY_EWMA_ALPHA: f64 = 0.3;

fn cost_multiplier(provider: &Provider) -> f64 {
    provider
        .meta
        .as_ref()
        .and_then(|m| m.cost_multiplier.as_deref())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(1.0)
}

/// Why a provider was not attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcurrencyLimited {
    pub limit: u32,
}

impl RoutingState {
    pub fn new() -> Self {
        Self::default()
    }

    /// The app's routing config, re-read from the database at most every
    /// [`CONFIG_CACHE_TTL`]. Read errors fall back to the default config.
    pub async fn config_for(&self, db: &Arc<Database>, app_type: &str) -> Arc<RoutingConfig> {
        if let Some((config, loaded)) = self
            .configs
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(app_type)
        {
            if loaded.elapsed() < CONFIG_CACHE_TTL {
                return config.clone();
            }
        }
        let db = db.clone();
        let app = app_type.to_string();
        let loaded = tokio::task::spawn_blocking(move || load_routing_config(&db, &app))
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        let config = Arc::new(loaded);
        self.configs
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(app_type.to_string(), (config.clone(), Instant::now()));
        config
    }

    /// Reorder the selected (available) providers for this request.
    ///
    /// `session_id` must be a client-provided id; generated ids are unique
    /// per request and would only churn the sticky map.
    #[cfg(test)]
    pub fn order(
        &self,
        app_type: &str,
        config: &RoutingConfig,
        session_id: Option<&str>,
        providers: Vec<Provider>,
    ) -> Vec<Provider> {
        self.route(app_type, config, session_id, "", providers)
            .providers
    }

    /// Route one request for `model`: apply the first matching model rule,
    /// then the strategy and sticky pin (PRODUCT-13).
    pub fn route(
        &self,
        app_type: &str,
        config: &RoutingConfig,
        session_id: Option<&str>,
        model: &str,
        providers: Vec<Provider>,
    ) -> RoutingDecision {
        let mut decision = RoutingDecision {
            providers,
            rule: None,
            rule_fell_back: false,
            strategy: config.strategy,
        };
        if config.is_default_behaviour() {
            return decision;
        }
        if let Some(rule) = config.rule_for(model) {
            decision.rule = Some(rule.label().to_string());
            if let Some(strategy) = rule.strategy {
                decision.strategy = strategy;
            }
            if !rule.providers.is_empty() {
                let candidates: Vec<Provider> = rule
                    .providers
                    .iter()
                    .filter_map(|id| decision.providers.iter().find(|p| &p.id == id).cloned())
                    .collect();
                if !candidates.is_empty() || rule.exclusive {
                    decision.providers = candidates;
                } else {
                    decision.rule_fell_back = true;
                }
            }
        }
        let providers = std::mem::take(&mut decision.providers);
        decision.providers =
            self.apply_strategy(app_type, config, decision.strategy, session_id, providers);
        decision
    }

    fn apply_strategy(
        &self,
        app_type: &str,
        config: &RoutingConfig,
        strategy: RoutingStrategy,
        session_id: Option<&str>,
        mut providers: Vec<Provider>,
    ) -> Vec<Provider> {
        if providers.len() <= 1 {
            return providers;
        }
        // Whole-list orderings: the failover chain follows the same metric.
        // Stable sorts keep queue order among equals.
        if strategy == RoutingStrategy::LeastLatency {
            let latency = self.latency.lock().unwrap_or_else(|e| e.into_inner());
            let key = |p: &Provider| {
                latency
                    .get(&format!("{app_type}:{}", p.id))
                    .copied()
                    .unwrap_or(0.0)
            };
            providers.sort_by(|a, b| key(a).total_cmp(&key(b)));
        } else if strategy == RoutingStrategy::Cheapest {
            providers.sort_by(|a, b| cost_multiplier(a).total_cmp(&cost_multiplier(b)));
        }

        let first = match strategy {
            RoutingStrategy::Priority
            | RoutingStrategy::LeastLatency
            | RoutingStrategy::Cheapest => 0,
            RoutingStrategy::RoundRobin => {
                let mut counters = self.round_robin.lock().unwrap_or_else(|e| e.into_inner());
                let counter = counters.entry(app_type.to_string()).or_insert(0);
                let index = *counter % providers.len();
                *counter = counter.wrapping_add(1);
                index
            }
            RoutingStrategy::Weighted => self.pick_weighted(app_type, config, &providers),
        };
        if first > 0 {
            let chosen = providers.remove(first);
            providers.insert(0, chosen);
        }

        if config.sticky_sessions {
            if let Some(pinned) = session_id.and_then(|id| self.pinned(app_type, config, id)) {
                if let Some(index) = providers.iter().position(|p| p.id == pinned) {
                    if index > 0 {
                        let chosen = providers.remove(index);
                        providers.insert(0, chosen);
                    }
                }
            }
        }
        providers
    }

    /// Smooth weighted round-robin: add each weight to its current value,
    /// pick the largest, subtract the total from the winner.
    fn pick_weighted(
        &self,
        app_type: &str,
        config: &RoutingConfig,
        providers: &[Provider],
    ) -> usize {
        let mut all = self.weighted.lock().unwrap_or_else(|e| e.into_inner());
        let current = all.entry(app_type.to_string()).or_default();
        current.retain(|id, _| providers.iter().any(|p| &p.id == id));
        let total: i64 = providers.iter().map(|p| config.weight_of(&p.id)).sum();
        if total <= 0 {
            return 0;
        }
        let mut best: Option<(usize, i64)> = None;
        for (index, provider) in providers.iter().enumerate() {
            let weight = config.weight_of(&provider.id);
            let value = current.entry(provider.id.clone()).or_insert(0);
            *value += weight;
            if weight > 0 && best.is_none_or(|(_, v)| *value > v) {
                best = Some((index, *value));
            }
        }
        let Some((index, _)) = best else {
            return 0;
        };
        if let Some(value) = current.get_mut(&providers[index].id) {
            *value -= total;
        }
        index
    }

    fn pinned(&self, app_type: &str, config: &RoutingConfig, session_id: &str) -> Option<String> {
        let mut sticky = self.sticky.lock().unwrap_or_else(|e| e.into_inner());
        let key = (app_type.to_string(), session_id.to_string());
        match sticky.get(&key) {
            Some(entry) if entry.last_used.elapsed() < config.sticky_ttl() => {
                Some(entry.provider_id.clone())
            }
            Some(_) => {
                sticky.remove(&key);
                None
            }
            None => None,
        }
    }

    /// Remember which provider served a session (no-op unless sticky is on).
    pub fn record_served(
        &self,
        app_type: &str,
        config: &RoutingConfig,
        session_id: Option<&str>,
        provider_id: &str,
    ) {
        let Some(session_id) = session_id.filter(|s| !s.is_empty()) else {
            return;
        };
        if !config.sticky_sessions {
            return;
        }
        let mut sticky = self.sticky.lock().unwrap_or_else(|e| e.into_inner());
        let key = (app_type.to_string(), session_id.to_string());
        if !sticky.contains_key(&key) && sticky.len() >= MAX_STICKY_SESSIONS {
            let ttl = config.sticky_ttl();
            sticky.retain(|_, entry| entry.last_used.elapsed() < ttl);
            if sticky.len() >= MAX_STICKY_SESSIONS {
                if let Some(oldest) = sticky
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_used)
                    .map(|(key, _)| key.clone())
                {
                    sticky.remove(&oldest);
                }
            }
        }
        sticky.insert(
            key,
            StickyEntry {
                provider_id: provider_id.to_string(),
                last_used: Instant::now(),
            },
        );
    }

    /// Feed one successful attempt's latency into the EWMA used by
    /// [`RoutingStrategy::LeastLatency`].
    pub fn record_latency(&self, app_type: &str, provider_id: &str, elapsed: Duration) {
        let ms = elapsed.as_secs_f64() * 1000.0;
        let mut latency = self.latency.lock().unwrap_or_else(|e| e.into_inner());
        let entry = latency
            .entry(format!("{app_type}:{provider_id}"))
            .or_insert(ms);
        *entry = LATENCY_EWMA_ALPHA * ms + (1.0 - LATENCY_EWMA_ALPHA) * *entry;
    }

    /// Current latency estimate in ms (`None` before the first sample).
    #[cfg(test)]
    pub fn latency_ms(&self, app_type: &str, provider_id: &str) -> Option<f64> {
        self.latency
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&format!("{app_type}:{provider_id}"))
            .copied()
    }

    /// Number of remembered session pins.
    #[cfg(test)]
    pub fn sticky_len(&self) -> usize {
        self.sticky.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Take an in-flight slot for `provider_id`. `Ok(None)` when unlimited.
    pub fn try_acquire(
        &self,
        app_type: &str,
        config: &RoutingConfig,
        provider_id: &str,
    ) -> Result<Option<OwnedSemaphorePermit>, ConcurrencyLimited> {
        let limit = config.max_concurrent_per_provider;
        if limit == 0 {
            return Ok(None);
        }
        let semaphore = {
            let mut limits = self.limits.lock().unwrap_or_else(|e| e.into_inner());
            let entry = limits
                .entry(format!("{app_type}:{provider_id}"))
                .or_insert_with(|| LimitEntry {
                    limit,
                    semaphore: Arc::new(Semaphore::new(limit as usize)),
                });
            if entry.limit != limit {
                // A changed limit starts a fresh semaphore; permits held on the
                // old one drain naturally.
                *entry = LimitEntry {
                    limit,
                    semaphore: Arc::new(Semaphore::new(limit as usize)),
                };
            }
            entry.semaphore.clone()
        };
        semaphore
            .try_acquire_owned()
            .map(Some)
            .map_err(|_| ConcurrencyLimited { limit })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(id: &str) -> Provider {
        Provider::with_id(id.to_string(), id.to_string(), json!({}), None)
    }

    fn ids(providers: &[Provider]) -> Vec<&str> {
        providers.iter().map(|p| p.id.as_str()).collect()
    }

    fn three() -> Vec<Provider> {
        vec![provider("p1"), provider("p2"), provider("p3")]
    }

    #[test]
    fn default_config_keeps_queue_order() {
        let state = RoutingState::new();
        let config = RoutingConfig::default();
        assert!(config.is_default_behaviour());
        for _ in 0..3 {
            let ordered = state.order("claude", &config, Some("s1"), three());
            assert_eq!(ids(&ordered), ["p1", "p2", "p3"]);
        }
        state.record_served("claude", &config, Some("s1"), "p2");
        assert_eq!(state.sticky_len(), 0, "sticky off records nothing");
        assert!(state
            .try_acquire("claude", &config, "p1")
            .unwrap()
            .is_none());
    }

    #[test]
    fn round_robin_rotates_the_first_provider_and_keeps_the_rest_in_order() {
        let state = RoutingState::new();
        let config = RoutingConfig {
            strategy: RoutingStrategy::RoundRobin,
            ..Default::default()
        };
        let firsts: Vec<String> = (0..6)
            .map(|_| state.order("claude", &config, None, three())[0].id.clone())
            .collect();
        assert_eq!(firsts, ["p1", "p2", "p3", "p1", "p2", "p3"]);
        let ordered = state.order("claude", &config, None, three());
        assert_eq!(ids(&ordered), ["p1", "p2", "p3"]);
        let ordered = state.order("claude", &config, None, three());
        assert_eq!(ids(&ordered), ["p2", "p1", "p3"]);
    }

    #[test]
    fn weighted_distributes_first_choice_by_weight() {
        let state = RoutingState::new();
        let config = RoutingConfig {
            strategy: RoutingStrategy::Weighted,
            weights: HashMap::from([
                ("p1".to_string(), 3),
                ("p2".to_string(), 1),
                ("p3".to_string(), 0),
            ]),
            ..Default::default()
        };
        let mut counts: HashMap<String, usize> = HashMap::new();
        for _ in 0..40 {
            let first = state.order("claude", &config, None, three())[0].id.clone();
            *counts.entry(first).or_default() += 1;
        }
        assert_eq!(counts.get("p1"), Some(&30));
        assert_eq!(counts.get("p2"), Some(&10));
        assert_eq!(counts.get("p3"), None, "weight 0 is never first");
    }

    #[test]
    fn sticky_session_prefers_the_pinned_provider_until_ttl() {
        let state = RoutingState::new();
        let config = RoutingConfig {
            sticky_sessions: true,
            sticky_ttl_secs: 3600,
            ..Default::default()
        };
        // P1 failed earlier, P2 served the session.
        state.record_served("claude", &config, Some("s1"), "p2");
        let ordered = state.order("claude", &config, Some("s1"), three());
        assert_eq!(
            ids(&ordered),
            ["p2", "p1", "p3"],
            "P1 recovered but s1 stays on P2"
        );
        // Other sessions and other apps are unaffected.
        assert_eq!(
            state.order("claude", &config, Some("s2"), three())[0].id,
            "p1"
        );
        assert_eq!(
            state.order("codex", &config, Some("s1"), three())[0].id,
            "p1"
        );
        // A pinned provider that is not selectable (breaker open) is skipped.
        let ordered = state.order(
            "claude",
            &config,
            Some("s1"),
            vec![provider("p1"), provider("p3")],
        );
        assert_eq!(ids(&ordered), ["p1", "p3"]);

        // Expired pins are ignored and dropped.
        let expired = RoutingConfig {
            sticky_ttl_secs: 1,
            ..config.clone()
        };
        {
            let mut sticky = state.sticky.lock().unwrap();
            sticky
                .get_mut(&("claude".to_string(), "s1".to_string()))
                .unwrap()
                .last_used = Instant::now() - Duration::from_secs(5);
        }
        assert_eq!(
            state.order("claude", &expired, Some("s1"), three())[0].id,
            "p1"
        );
        assert_eq!(state.sticky_len(), 0);
    }

    #[test]
    fn sticky_map_is_bounded() {
        let state = RoutingState::new();
        let config = RoutingConfig {
            sticky_sessions: true,
            ..Default::default()
        };
        for i in 0..(MAX_STICKY_SESSIONS + 10) {
            state.record_served("claude", &config, Some(&format!("s{i}")), "p1");
        }
        assert_eq!(state.sticky_len(), MAX_STICKY_SESSIONS);
    }

    #[test]
    fn concurrency_limit_rejects_beyond_the_cap_and_releases_on_drop() {
        let state = RoutingState::new();
        let config = RoutingConfig {
            max_concurrent_per_provider: 2,
            ..Default::default()
        };
        let a = state.try_acquire("claude", &config, "p1").unwrap();
        let b = state.try_acquire("claude", &config, "p1").unwrap();
        assert!(a.is_some() && b.is_some());
        assert_eq!(
            state.try_acquire("claude", &config, "p1").unwrap_err(),
            ConcurrencyLimited { limit: 2 }
        );
        assert!(
            state.try_acquire("claude", &config, "p2").is_ok(),
            "per provider"
        );
        drop(a);
        assert!(state.try_acquire("claude", &config, "p1").is_ok());
        drop(b);
    }

    #[test]
    fn configs_round_trip_through_the_settings_table() {
        let db = Database::memory().expect("memory db");
        assert_eq!(
            load_routing_config(&db, "claude").unwrap(),
            RoutingConfig::default()
        );
        let config = RoutingConfig {
            strategy: RoutingStrategy::Weighted,
            sticky_sessions: true,
            sticky_ttl_secs: 60,
            max_concurrent_per_provider: 4,
            weights: HashMap::from([("p1".to_string(), 2)]),
            model_rules: vec![ModelRoutingRule {
                pattern: "claude-haiku-*".to_string(),
                providers: vec!["p2".to_string()],
                strategy: Some(RoutingStrategy::Cheapest),
                ..Default::default()
            }],
        };
        save_routing_config(&db, "claude", &config).unwrap();
        save_routing_config(
            &db,
            "codex",
            &RoutingConfig {
                sticky_sessions: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(load_routing_config(&db, "claude").unwrap(), config);
        assert!(load_routing_config(&db, "codex").unwrap().sticky_sessions);
        // Saving the default removes the entry.
        save_routing_config(&db, "codex", &RoutingConfig::default()).unwrap();
        assert_eq!(load_routing_configs(&db).unwrap().len(), 1);
        // camelCase wire format, unknown fields tolerated.
        let parsed: RoutingConfig = serde_json::from_value(json!({
            "strategy": "roundRobin",
            "stickySessions": true,
            "maxConcurrentPerProvider": 3,
            "future": 1
        }))
        .unwrap();
        assert_eq!(parsed.strategy, RoutingStrategy::RoundRobin);
        assert_eq!(parsed.max_concurrent_per_provider, 3);
    }

    fn priced(id: &str, multiplier: Option<&str>) -> Provider {
        let mut p = provider(id);
        if let Some(m) = multiplier {
            p.meta = Some(crate::provider::ProviderMeta {
                cost_multiplier: Some(m.to_string()),
                ..Default::default()
            });
        }
        p
    }

    #[test]
    fn glob_matches_star_patterns_case_insensitively() {
        assert!(glob_match("claude-haiku-*", "claude-haiku-4-5"));
        assert!(glob_match("*opus*", "claude-opus-4-8"));
        assert!(glob_match("gpt-5", "gpt-5"));
        assert!(!glob_match("gpt-5", "gpt-5-mini"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("a*b", "acd"));
        let rule = ModelRoutingRule {
            pattern: "Claude-Haiku-*".to_string(),
            ..Default::default()
        };
        assert!(rule.matches("claude-HAIKU-4"));
        assert_eq!(rule.label(), "Claude-Haiku-*");
    }

    #[test]
    fn haiku_rule_routes_to_the_cheapest_candidate_and_names_the_rule() {
        let state = RoutingState::new();
        let config = RoutingConfig {
            model_rules: vec![
                ModelRoutingRule {
                    name: "haiku → cheapest".to_string(),
                    pattern: "claude-haiku-*".to_string(),
                    strategy: Some(RoutingStrategy::Cheapest),
                    ..Default::default()
                },
                ModelRoutingRule {
                    pattern: "claude-opus-*".to_string(),
                    providers: vec!["official".to_string()],
                    exclusive: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert!(!config.is_default_behaviour());
        let providers = vec![
            priced("official", None),
            priced("relay-a", Some("0.8")),
            priced("relay-b", Some("0.3")),
        ];
        let d = state.route(
            "claude",
            &config,
            None,
            "claude-haiku-4-5",
            providers.clone(),
        );
        assert_eq!(d.rule.as_deref(), Some("haiku → cheapest"));
        assert_eq!(ids(&d.providers), ["relay-b", "relay-a", "official"]);

        let d = state.route(
            "claude",
            &config,
            None,
            "claude-opus-4-8",
            providers.clone(),
        );
        assert_eq!(ids(&d.providers), ["official"]);
        // An exclusive rule whose provider is unavailable yields no candidates.
        let d = state.route(
            "claude",
            &config,
            None,
            "claude-opus-4-8",
            providers[1..].to_vec(),
        );
        assert!(d.providers.is_empty());

        // No rule matches: default priority order, no rule reported.
        let d = state.route("claude", &config, None, "claude-sonnet-4", providers);
        assert_eq!(d.rule, None);
        assert_eq!(ids(&d.providers), ["official", "relay-a", "relay-b"]);
    }

    #[test]
    fn non_exclusive_rule_falls_back_when_its_providers_are_unavailable() {
        let state = RoutingState::new();
        let config = RoutingConfig {
            model_rules: vec![ModelRoutingRule {
                pattern: "gpt-*".to_string(),
                providers: vec!["missing".to_string(), "p3".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let d = state.route("codex", &config, None, "gpt-5", three());
        assert_eq!(ids(&d.providers), ["p3"], "candidates keep rule order");
        assert!(!d.rule_fell_back);
        let d = state.route(
            "codex",
            &config,
            None,
            "gpt-5",
            vec![provider("p1"), provider("p2")],
        );
        assert!(d.rule_fell_back);
        assert_eq!(ids(&d.providers), ["p1", "p2"]);
    }

    #[test]
    fn least_latency_prefers_unmeasured_then_fastest() {
        let state = RoutingState::new();
        let config = RoutingConfig {
            strategy: RoutingStrategy::LeastLatency,
            ..Default::default()
        };
        state.record_latency("claude", "p1", Duration::from_millis(900));
        state.record_latency("claude", "p2", Duration::from_millis(200));
        let ordered = state.order("claude", &config, None, three());
        assert_eq!(ids(&ordered), ["p3", "p2", "p1"]);
        state.record_latency("claude", "p3", Duration::from_millis(500));
        let ordered = state.order("claude", &config, None, three());
        assert_eq!(ids(&ordered), ["p2", "p3", "p1"]);
        // EWMA: one slow sample moves the estimate only partway.
        state.record_latency("claude", "p2", Duration::from_millis(1200));
        let est = state.latency_ms("claude", "p2").unwrap();
        assert!((est - 500.0).abs() < 1.0, "{est}");
        // Other apps keep their own estimates.
        assert_eq!(state.latency_ms("codex", "p2"), None);
    }

    #[test]
    fn unparsable_setting_falls_back_to_default() {
        let db = Database::memory().expect("memory db");
        db.set_setting(ROUTING_SETTINGS_KEY, "not json").unwrap();
        assert_eq!(
            load_routing_config(&db, "claude").unwrap(),
            RoutingConfig::default()
        );
    }
}
