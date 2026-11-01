//! The proxy request pipeline (PROXY-07).
//!
//! One client request flows through these stages:
//!
//! 1. **parse** ([`inbound`]): read and decode the body, parse JSON, derive the
//!    endpoint (handlers).
//! 2. **route/select**: `RequestContext` loads the config snapshot and picks the
//!    failover queue; [`timeout::TimeoutPolicy`] is resolved once.
//! 3. **per attempt** (`RequestForwarder`, one per provider):
//!    budget check → breaker permit → [`endpoint::Route`] →
//!    body ([`body`]: model mapping, optimizers, protocol transform, outbound
//!    policy) → upstream URL ([`endpoint`]) → credentials ([`auth`]) →
//!    headers ([`headers::HeaderPolicy`]) → send ([`transport`]) →
//!    classify ([`validate`]) → [`attempt::AttemptOutcome`] → record
//!    (breaker, [`metrics::ProxyMetrics`]).
//! 4. **respond**: `response_processor` / protocol response transforms, usage
//!    logging through `UsageLogTarget`, stream health reporting (PROXY-09).
//!
//! Every stage except the managed-account token lookup in [`auth`] works without
//! a Tauri `AppHandle`, so each is unit-tested in isolation.

pub mod attempt;
pub mod auth;
pub mod body;
pub mod endpoint;
pub mod headers;
pub mod inbound;
pub mod metrics;
pub mod timeout;
pub mod transport;
pub mod validate;
