//! Read-only proxy dashboard (PRODUCT-30, first increment).
//!
//! - `GET /dashboard` serves one self-contained HTML page (no external assets,
//!   strict CSP) that polls the two endpoints below and renders the proxy
//!   status, active targets and the last requests with their attempt hops and
//!   trace (PRODUCT-57).
//! - `GET /api/status` returns the full [`ProxyStatus`].
//! - `GET /api/requests?limit=N` returns the newest flight-recorder records
//!   (default 100, max [`flight_recorder::CAPACITY`]).
//!
//! The API endpoints answer token-authenticated callers and peers on this
//! machine only (like `/metrics`); everyone else gets 401. The page itself
//! carries no data. A LAN client enters the access token in the page, which
//! sends it as `Authorization: Bearer` and keeps it in `sessionStorage`.
//! Nothing here can change state.

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use serde::Deserialize;

use super::super::flight_recorder;
use super::access_guard::{AccessContext, PeerInfo};
use super::ProxyState;

/// Default number of records `/api/requests` returns.
pub const DEFAULT_REQUEST_LIMIT: usize = 100;

const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

fn allowed(ctx: Option<Extension<AccessContext>>, peer: Option<Extension<PeerInfo>>) -> bool {
    let authenticated = ctx.is_some_and(|Extension(ctx)| ctx.token_authenticated);
    let local = peer.is_some_and(|Extension(p)| p.remote.ip().to_canonical().is_loopback());
    authenticated || local
}

fn unauthorized() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(
            serde_json::json!({ "error": "the dashboard API requires the CC Switch access token" }),
        ),
    )
        .into_response()
}

/// `GET /dashboard`.
pub async fn page() -> axum::response::Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CONTENT_SECURITY_POLICY, CSP),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::REFERRER_POLICY, "no-referrer"),
        ],
        DASHBOARD_HTML,
    )
        .into_response()
}

/// `GET /api/status`.
pub async fn api_status(
    State(state): State<ProxyState>,
    ctx: Option<Extension<AccessContext>>,
    peer: Option<Extension<PeerInfo>>,
) -> axum::response::Response {
    if !allowed(ctx, peer) {
        return unauthorized();
    }
    Json(state.status_snapshot().await).into_response()
}

#[derive(Debug, Deserialize)]
pub struct RequestsQuery {
    limit: Option<usize>,
}

/// `GET /api/requests`.
pub async fn api_requests(
    ctx: Option<Extension<AccessContext>>,
    peer: Option<Extension<PeerInfo>>,
    Query(query): Query<RequestsQuery>,
) -> axum::response::Response {
    if !allowed(ctx, peer) {
        return unauthorized();
    }
    let limit = query
        .limit
        .unwrap_or(DEFAULT_REQUEST_LIMIT)
        .clamp(1, flight_recorder::CAPACITY);
    let mut records = flight_recorder::global().recent();
    records.truncate(limit);
    Json(records).into_response()
}

/// The page. Every value is inserted with `textContent`, never as HTML.
pub const DASHBOARD_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>CC Switch Proxy</title>
<style>
:root { --bg:#fafafa; --fg:#1d1d1f; --muted:#6e6e73; --line:#e5e5ea; --ok:#1a7f37; --bad:#c62828; --card:#fff; }
@media (prefers-color-scheme: dark) { :root { --bg:#151517; --fg:#f2f2f7; --muted:#a1a1a6; --line:#2c2c2e; --ok:#3fb950; --bad:#ff6b6b; --card:#1c1c1e; } }
* { box-sizing: border-box; }
body { margin:0; padding:16px; background:var(--bg); color:var(--fg); font:14px/1.45 system-ui, -apple-system, "Segoe UI", sans-serif; }
h1 { font-size:18px; margin:0 0 12px; }
h2 { font-size:15px; margin:20px 0 8px; }
.card { background:var(--card); border:1px solid var(--line); border-radius:8px; padding:12px; overflow-x:auto; }
.grid { display:grid; grid-template-columns:repeat(auto-fit, minmax(140px, 1fr)); gap:8px; }
.k { color:var(--muted); font-size:12px; }
.v { font-size:16px; font-variant-numeric:tabular-nums; }
table { width:100%; border-collapse:collapse; font-size:13px; }
th, td { text-align:left; padding:6px 8px; border-bottom:1px solid var(--line); vertical-align:top; }
th { color:var(--muted); font-weight:500; }
.ok { color:var(--ok); } .bad { color:var(--bad); } .muted { color:var(--muted); }
#auth { display:none; margin-bottom:12px; }
input { padding:6px 8px; border:1px solid var(--line); border-radius:6px; background:var(--card); color:var(--fg); }
button { padding:6px 10px; border:1px solid var(--line); border-radius:6px; background:var(--card); color:var(--fg); cursor:pointer; }
</style>
</head>
<body>
<h1>CC Switch proxy</h1>
<div id="auth" class="card">
  <div class="k">This proxy is not on this machine: enter its access token.</div>
  <input id="token" type="password" autocomplete="off" aria-label="Access token">
  <button id="save" type="button">Connect</button>
</div>
<div class="card"><div class="grid" id="summary"></div></div>
<h2>Active targets</h2>
<div class="card"><table><thead><tr><th>App</th><th>Provider</th></tr></thead><tbody id="targets"></tbody></table></div>
<h2>Recent requests</h2>
<div class="card"><table>
<thead><tr><th>Time</th><th>App</th><th>Endpoint</th><th>Result</th><th>ms</th><th>Attempts</th><th>Trace</th></tr></thead>
<tbody id="requests"></tbody></table></div>
<p class="muted" id="updated"></p>
<script>
(function () {
  var KEY = "cc-switch-dashboard-token";
  function token() { try { return sessionStorage.getItem(KEY) || ""; } catch (e) { return ""; } }
  function el(tag, text, cls) { var e = document.createElement(tag); if (text != null) e.textContent = String(text); if (cls) e.className = cls; return e; }
  function get(path) {
    var headers = {}; var t = token(); if (t) headers["Authorization"] = "Bearer " + t;
    return fetch(path, { headers: headers, cache: "no-store" }).then(function (r) {
      if (r.status === 401) { document.getElementById("auth").style.display = "block"; throw new Error("unauthorized"); }
      return r.json();
    });
  }
  function stat(label, value, cls) { var d = el("div"); d.appendChild(el("div", label, "k")); d.appendChild(el("div", value, "v " + (cls || ""))); return d; }
  function render(status, requests) {
    var s = document.getElementById("summary"); s.replaceChildren(
      stat("Status", status.running ? "running" : "stopped", status.running ? "ok" : "bad"),
      stat("Port", status.port),
      stat("Requests", status.total_requests),
      stat("Success rate", (Number(status.success_rate) || 0).toFixed(1) + "%"),
      stat("Active connections", status.active_connections),
      stat("Failovers", status.failover_count));
    var tb = document.getElementById("targets"); tb.replaceChildren();
    (status.active_targets || []).forEach(function (t) { var tr = el("tr"); tr.appendChild(el("td", t.app_type)); tr.appendChild(el("td", t.provider_name)); tb.appendChild(tr); });
    var rb = document.getElementById("requests"); rb.replaceChildren();
    (requests || []).forEach(function (r) {
      var tr = el("tr");
      tr.appendChild(el("td", new Date(r.startedAt).toLocaleTimeString()));
      tr.appendChild(el("td", r.appType));
      tr.appendChild(el("td", r.endpoint));
      tr.appendChild(el("td", r.success ? "ok" : "failed", r.success ? "ok" : "bad"));
      tr.appendChild(el("td", r.durationMs));
      tr.appendChild(el("td", (r.attempts || []).map(function (a) { return a.providerName + " " + a.result + (a.status ? " " + a.status : ""); }).join(" → ")));
      tr.appendChild(el("td", (r.trace || []).map(function (t) { return t.kind + ": " + t.detail; }).join("; "), "muted"));
      rb.appendChild(tr);
    });
    document.getElementById("updated").textContent = "Updated " + new Date().toLocaleTimeString();
  }
  function refresh() {
    Promise.all([get("/api/status"), get("/api/requests?limit=100")]).then(function (v) {
      document.getElementById("auth").style.display = "none"; render(v[0], v[1]);
    }).catch(function () {});
  }
  document.getElementById("save").addEventListener("click", function () {
    try { sessionStorage.setItem(KEY, document.getElementById("token").value.trim()); } catch (e) {}
    refresh();
  });
  refresh(); setInterval(refresh, 5000);
})();
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_inserts_values_as_text_only() {
        assert!(!DASHBOARD_HTML.contains("innerHTML"));
        assert!(!DASHBOARD_HTML.contains("http://") && !DASHBOARD_HTML.contains("https://"));
        assert!(DASHBOARD_HTML.contains("/api/requests"));
    }
}
