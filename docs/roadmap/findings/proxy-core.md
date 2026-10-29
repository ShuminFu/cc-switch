# Proxy core — findings (PROXY-xx)

Part of the [next-phase proposals](../README.md). 48 findings. Priorities: P1 = 1, P2 = 27, P3 = 20.

## Area summary

The proxy core (server, forwarder, router, circuit breaker, response processor and the request-rectifier/optimizer helpers) is functionally rich and unusually well commented, with strong unit coverage of pure helpers (SSE parsing, content-encoding, URL rewriting, rectifiers, JSON canonicalization). The in-memory stores that grow with traffic (Gemini shadow, Codex chat history) are bounded, and the connection/usage guards are RAII-based. The main risks are structural and operational: `forwarder.rs` (4.8k lines, one ~1.1k-line `forward()` function) and `handlers.rs` (3.1k lines, five near-identical handler bodies, usage logging re-implemented ~6 times) carry all protocol, auth, header, timeout and transport decisions in one place, which is why several correctness gaps exist in the seams: a same-provider duplicate send in the hyper fallback, a HalfOpen permit that leaks when a client aborts (provider stuck until manual reset), a raw TCP+TLS connection per request with no connect timeout (an unreachable provider stalls failover for the full non-streaming timeout), and blocking SQLite calls on every request inside the async runtime. Security posture is adequate for the default loopback bind, but the UI allows `0.0.0.0` with no authentication on the main routes, and debug/error logs persist full prompts, response bodies, and upstream headers to the on-disk log file. Test coverage of the end-to-end failover/stream paths is effectively zero (no mock upstream, `forward()` requires a Tauri `AppHandle`). Suggested direction for the next phase: (1) fix the four P1 correctness/perf bugs with RAII guards and a single transport/timeout layer; (2) introduce an integration test harness with a mock upstream before (3) decomposing the forwarder into a staged request pipeline + pluggable transport + routing strategy, which also unlocks the requested features (sticky sessions, load balancing, retry budgets, metrics/tracing, flight recorder).

## Index

| ID | Title | Type | Priority | Effort | Confidence | Verdict |
|---|---|---|---|---|---|---|
| [PROXY-03](#proxy-03) | Proxy can be bound to all interfaces with no authentication on the main API routes | security | P1 | M | high | confirmed |
| [PROXY-01](#proxy-01) | Hyper raw-write fallback re-sends the request after it was already written to the upstream | bug | P2 (first pass P1) | M | high | confirmed |
| [PROXY-02](#proxy-02) | HalfOpen probe permit leaks when the client disconnects mid-request, leaving the provider stuck in HalfOpen | bug | P2 (first pass P1) | M | high | partially-confirmed |
| [PROXY-04](#proxy-04) | Blocking SQLite (std Mutex + sync I/O) on the async hot path, 6-10 times per request | performance | P2 (first pass P1) | L | medium | partially-confirmed |
| [PROXY-05](#proxy-05) | Native-Anthropic path opens a new TCP+TLS connection per request; the raw-write mechanism is likely redundant | performance | P2 (first pass P1) | M | medium | partially-confirmed |
| [PROXY-06](#proxy-06) | No end-to-end tests of the proxy against a mock upstream; `forward()` is untestable | test | P2 (first pass P1) | L | high | partially-confirmed |
| [PROXY-07](#proxy-07) | Decompose `forwarder.rs`/`handlers.rs` into a staged request pipeline with pluggable transport | refactor | P2 (first pass P1) | XL | high | partially-confirmed |
| [PROXY-08](#proxy-08) | Timeout policy is split across three layers and the raw path has no connect timeout, so an unreachable provider stalls failover for the full non-streaming timeout | bug | P2 | M | high |  |
| [PROXY-09](#proxy-09) | Streams that fail after the first chunk never feed back into the breaker/health, and idle timeouts just truncate the client stream | bug | P2 | M | medium |  |
| [PROXY-10](#proxy-10) | Circuit breaker error-rate uses lifetime counters (no sliding window) and defaults disagree | bug | P2 | S | high |  |
| [PROXY-11](#proxy-11) | A single-provider failover queue causes a 60 s total outage once the breaker opens | bug | P2 | S | medium |  |
| [PROXY-12](#proxy-12) | Thinking-signature rectifier fires on any upstream error containing "invalid request" and strips all thinking context | bug | P2 | S | medium |  |
| [PROXY-13](#proxy-13) | Full prompts, response bodies and all upstream headers are written to the persistent log file | security | P2 | S | high |  |
| [PROXY-14](#proxy-14) | Server stop does not drain connections, `ProxyServer` has no `Drop`, and `StopTimeout` is treated as fatal by config updates | bug | P2 | M | high |  |
| [PROXY-15](#proxy-15) | No per-connection timeouts or limits on the accept side; `peek` can block a task forever | security | P2 | S | medium |  |
| [PROXY-16](#proxy-16) | `ProviderRouter` holds the breaker-map write lock across a blocking DB call | performance | P2 | S | high |  |
| [PROXY-17](#proxy-17) | Every request body is fully buffered, parsed to `serde_json::Value`, cloned per attempt and re-serialized with canonical key order | performance | P2 | L | medium |  |
| [PROXY-18](#proxy-18) | Usage logging is re-implemented six times with divergent rules | refactor | P2 | M | high |  |
| [PROXY-19](#proxy-19) | Routing is a fixed priority list; no sticky sessions, load balancing, per-provider concurrency/rate limits, or retry budget | feature | P2 | L | high |  |
| [PROXY-20](#proxy-20) | No metrics endpoint, tracing spans, or OpenTelemetry export | feature | P2 | L | high |  |
| [PROXY-21](#proxy-21) | No request flight recorder, replay, or export for debugging failed requests | feature | P2 | M | high |  |
| [PROXY-22](#proxy-22) | No active health probing; breaker recovery always burns a real user request (and `health.rs` is a stub) | feature | P2 | M | high |  |
| [PROXY-35](#proxy-35) | No developer or user documentation of the proxy pipeline, failover semantics, timeout rules, or header policy | docs | P2 | M | high |  |
| [PROXY-37](#proxy-37) | Underscore-key filter deletes legitimate keys inside tool_use inputs, tool results, Gemini function args and metadata on every upstream body | bug | P2 | S | medium |  |
| [PROXY-38](#proxy-38) | Prefixed client routes /claude/* and /gemini/* are forwarded to the upstream with the prefix intact (404 on passthrough) | bug | P2 | S | high |  |
| [PROXY-39](#proxy-39) | Emptying the failover queue (or deleting its last provider) while failover is enabled turns every request into 503 'no providers configured' | bug | P2 | S | high |  |
| [PROXY-40](#proxy-40) | Trust store differs per HTTP stack: corporate/MITM CAs work for native Claude (raw path) but not for Codex, Gemini, Copilot or the fallback client; no custom-CA option | bug | P2 | S | high |  |
| [PROXY-41](#proxy-41) | Native Anthropic/Gemini/Chat streaming paths have no semantic stream-start validation; a 2xx stream whose first event is an error is scored as success and never fails over | bug | P2 | M | medium |  |
| [PROXY-23](#proxy-23) | SSE usage collectors buffer every "usage"-matching event for the whole stream, and the passthrough SSE buffer is unbounded | performance | P3 | S | medium |  |
| [PROXY-24](#proxy-24) | Rectified request body is discarded when failing over to the next provider, doubling calls on every subsequent provider | bug | P3 | S | high |  |
| [PROXY-25](#proxy-25) | Request-side hop-by-hop headers (`connection`, `keep-alive`, `te`, `upgrade`, `expect`) are forwarded upstream | bug | P3 | S | high |  |
| [PROXY-26](#proxy-26) | Malformed client JSON yields HTTP 500 and is not logged as a client error | bug | P3 | S | high |  |
| [PROXY-27](#proxy-27) | `failover_count` counts "served provider != local current provider", not actual failovers, and switch tasks are spawned per request | bug | P3 | S | medium |  |
| [PROXY-28](#proxy-28) | Dead/stub modules and duplicated error→status mapping | tech-debt | P3 | M | high |  |
| [PROXY-29](#proxy-29) | `ProxyConfig` carries vestigial timeout/retry fields that the forwarder never reads | tech-debt | P3 | S | high |  |
| [PROXY-30](#proxy-30) | Takeover status and the stop-guard ignore Claude Desktop and Hermes routes | bug | P3 | S | medium |  |
| [PROXY-31](#proxy-31) | Error messages returned to API clients are Chinese and not machine-readable (outside the Codex path) | docs | P3 | S | high |  |
| [PROXY-32](#proxy-32) | Budget rectifier rewrites `max_tokens` to a hard-coded 64000 and budget to 32000 regardless of model limits | bug | P3 | S | medium |  |
| [PROXY-33](#proxy-33) | Upstream-proxy credentials are logged unmasked | security | P3 | S | high |  |
| [PROXY-34](#proxy-34) | `ProxyStatus` counters mutated under an async RwLock in 13 places with `success_rate` recomputed by hand each time | tech-debt | P3 | M | high |  |
| [PROXY-36](#proxy-36) | Circuit breaker and router unit tests miss error-rate, concurrency, hot-update and leak scenarios | test | P3 | S | high |  |
| [PROXY-42](#proxy-42) | Rectifier-retry failures use a different retryability rule than the main path: 429/401/403/404/408 and auth/config/transform errors abort failover and skip breaker accounting | bug | P3 | S | high |  |
| [PROXY-43](#proxy-43) | Header-case/order capture is per TCP connection, so every keep-alive request after the first is written with stale header casing and order | bug | P3 | S | high |  |
| [PROXY-44](#proxy-44) | Codex OAuth session headers are rewritten with the proxy's codex_-prefixed session id, replacing the client's real session_id value | bug | P3 | S | medium |  |
| [PROXY-45](#proxy-45) | Upstream response bodies are buffered and decompressed without any size cap; a gzip/zstd bomb from a hostile or buggy relay can exhaust the desktop app's memory | security | P3 | S | medium |  |
| [PROXY-46](#proxy-46) | mask_url byte-slices a non-URL string at index 20 and panics on multi-byte input, reachable from proxy-URL validation | bug | P3 | S | high |  |
| [PROXY-47](#proxy-47) | Bedrock thinking optimizer sets budget_tokens to max_tokens - 1, overrides the caller's smaller budget or disabled thinking, and forces effort=max plus the 1M beta for every adaptive model | bug | P3 | S | medium |  |
| [PROXY-48](#proxy-48) | Two write paths for auto_failover_enabled with different invariants: saving the per-app settings form can silently flip failover from a stale snapshot, bypassing the P1-switch and queue checks | bug | P3 | S | medium |  |

## Findings

### PROXY-03: Proxy can be bound to all interfaces with no authentication on the main API routes

**Type:** security · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/proxy/ProxyPanel.tsx:148-154` — `0.0.0.0` explicitly accepted; `src-tauri/src/proxy/server.rs:100-114` — binds whatever address is configured; `src-tauri/src/proxy/server.rs:291-370` — `/v1/messages`, `/v1/responses`, `/v1/chat/completions`, `/v1beta/*` have no auth layer; only `/claude-desktop/*` validate a bearer token (`src-tauri/src/proxy/handlers.rs:128`, `:144`, `:256-281`). `src-tauri/src/proxy/handlers.rs:68-71` — `/status` returns `last_error` (which embeds upstream error bodies, `forwarder.rs:1022-1025`) to anyone.
- **Problem:** On a LAN/VPN/hotspot, any host can spend the user's provider API keys, OAuth tokens (Copilot/Codex) and read `/status`. The proxy inserts real credentials server-side, so the attacker never needs a key.
- **Proposal:** When `listen_address` is not loopback, require `Authorization: Bearer <gateway token>` on every route (reuse `claude_desktop_config::get_or_create_gateway_token`) and write that token into the takeover configs (`ANTHROPIC_AUTH_TOKEN`/`OPENAI_API_KEY` placeholders already exist). Refuse to bind non-loopback unless the token mode is on; show a warning in the UI; strip `last_error` from `/status` unless authenticated.
- **Acceptance:** Integration test binding `127.0.0.1:0` with token mode forced: request without token → 401; with token → forwarded. Manual: bind `0.0.0.0`, confirm UI warning and 401 from another host.
- **Verification:** **confirmed** — ProxyPanel.tsx:150-154 accepts `0.0.0.0` and any IPv4/IPv6 literal, en.json:2518 only says "recommended: 127.0.0.1" with no warning, and neither commands/proxy.rs:79-84 nor services/proxy.rs:3033-3047 `update_config` validates the address before server.rs:100-114 binds it. `build_router` at server.rs:291-370 attaches only `DefaultBodyLimit`; the sole auth check is `validate_claude_desktop_gateway_auth` on the two `/claude-desktop/*` handlers (handlers.rs:128, :144, :256-281), so `/v1/messages`, `/v1/responses`, `/v1/chat/completions`, `/v1beta/*` relay the user's injected credentials to any network peer. handlers.rs:68-71 returns the whole `ProxyStatus` and forwarder.rs:1022-1025 stores `format!("Provider {} 失败: {}", name, e)` where `UpstreamError` Display (error.rs:42) embeds `{body:?}`, so `/status` leaks upstream error bodies unauthenticated. Default is loopback (types.rs:45, schema.rs:128) so exposure is opt-in, but WSL2/Docker users commonly bind 0.0.0.0 and there is no mitigation path, which keeps this at P1. The proposal is feasible (`get_or_create_gateway_token` at claude_desktop_config.rs:278, placeholders at services/proxy.rs:168-171/:990-993) and not implemented; note Gemini clients authenticate via `x-goog-api-key`/`?key=` rather than `Authorization`, which the gate must accept.

### PROXY-01: Hyper raw-write fallback re-sends the request after it was already written to the upstream

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/hyper_client.rs:268-291` — any `Err` from `send_raw_request` (except when a proxy URL is set) falls through to "Fallback: hyper-util Client" and sends the full request again; `src-tauri/src/proxy/hyper_client.rs:415-423` and `:428-436` — the raw request bytes (headers + body) are `write_all`+`flush`ed before `do_hyper_response`; `src-tauri/src/proxy/hyper_client.rs:666-702` — handshake/`send_request` failures after the write ("Handshake failed", "Response parse failed") are returned as `ForwardFailed` and therefore trigger the fallback.
- **Problem:** If the upstream accepted and billed the POST but reset the connection, returned an unparsable status line, or errored during response parsing, the proxy silently issues a second identical non-idempotent request to the same provider (double billing, duplicate side effects), on top of the failover loop which may retry on the next provider. The fallback client also uses a different trust store (`:59-73` webpki only vs `:564-579` webpki+native), so behavior differs between the two sends.
- **Proposal:** Split `send_raw_request` errors by phase (`Connect`/`Tls`/`Write` vs `AfterWrite`). Only fall back for pre-write failures; for post-write failures return `ForwardFailed` and let `forward_with_retry_inner` decide. Record a `duplicate_send_risk` counter in status when a retry happens after bytes were written. Unify the TLS root store between the two clients.
- **Acceptance:** Unit test with a fake TCP upstream that accepts the request, reads the body, then closes the socket: assert exactly one request is received. Test that a connect failure still falls back.
- **Verification:** **confirmed** — hyper_client.rs:268-291 falls through to the hyper-util client on any `Err` from `send_raw_request` unless `proxy_url.is_some()`, and :415-423/:428-436 `write_all`+`flush` the full request before `do_hyper_response`, whose `send_request` failure at :696-699 is a plain `ForwardFailed("Response parse failed")` that triggers that resend (the http1 `handshake` at :670-673 does no I/O, so only the post-write parse failure is realistic). The trust-store asymmetry is real: :63 `with_webpki_roots()` only vs :567-573 webpki + `rustls_native_certs`. Mitigations narrow the blast radius: the outer `tokio::time::timeout` at :267-273 returns `ProxyError::Timeout` via `?` so slow upstreams never double-send, the raw path opens a fresh TCP connection per request (no stale keep-alive), and `should_preserve_exact_header_case` at forwarder.rs:3141-3156 restricts the raw path to Anthropic-format providers (Codex/Gemini/Copilot/codex_oauth use reqwest). No test covers the fallback (rg "falling back" finds only the warn at :286). Downgraded to P2: a silent duplicate billed request on a rare upstream reset-before-headers, mostly affecting non-streaming calls, is medium for a desktop proxy; the proposal (phase-split errors, no fallback after write) is sound and not implemented.

### PROXY-02: HalfOpen probe permit leaks when the client disconnects mid-request, leaving the provider stuck in HalfOpen

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/circuit_breaker.rs:315-333` — only one HalfOpen permit; `:339-356` — released only by explicit calls; `:367-377` — `transition_to_half_open` resets the counter only from `Open`. `src-tauri/src/proxy/forwarder.rs:435-447` — permit acquired, then `self.forward(...).await` at `:479-491`; all releases (`:495`, `:681-687`, `:1010-1019`, `:1042-1048`, `:859-865`) happen after `.await` points. No `Drop` guard exists for the permit (`grep "impl Drop"` in forwarder/circuit_breaker only finds `ActiveConnectionGuard`).
- **Problem:** When the axum/hyper handler future is dropped (Claude Code Ctrl+C, client timeout, connection reset) while the probe request is in flight, `half_open_requests` stays at 1 forever. Every later `allow_request()` returns `allowed=false`, the router skips the provider, and if it is the only queue entry users get `NoAvailableProvider` (`forwarder.rs:1069-1084`) until a manual breaker reset.
- **Proposal:** Return an RAII `HalfOpenPermit` from `allow_request()` that releases on `Drop` (same pattern as `ActiveConnectionGuard`), with `record_success/failure` consuming it. Add a permit TTL (e.g. 2x non-streaming timeout) as defense in depth. Also apply the same cancellation-safety review to `status.failed_requests` accounting.
- **Acceptance:** Test: open breaker → HalfOpen → start a request that awaits a pending upstream → drop the future → assert `allow_request().allowed == true` afterwards.
- **Verification:** **partially-confirmed** — The leak mechanism is real: circuit_breaker.rs:317-333 holds a single permit in an AtomicU32, :339-356 releases only on explicit calls, :367-377 resets only from Open, and every forwarder release (:495 via record_success_result, :681-687, :859-865, :1010-1019, :1042-1048) sits after `self.forward(...).await` at :479-491 with no Drop guard (the only `impl Drop` in scope are `ActiveConnectionGuard` forwarder.rs:101 and `SseUsageFinishGuard` response_processor.rs:440); server.rs:166-206 runs each connection with `http1::Builder::serve_connection`, so a client disconnect drops the handler future mid-probe. Two things the finding gets wrong: forwarder.rs:412-413/:437-445 sets `bypass_circuit_breaker = providers.len() == 1` and skips `allow_provider_request` entirely, so a sole queue entry can never leak or hit `NoAvailableProvider`, and `auto_failover_enabled` defaults to 0 (schema.rs:130), in which case provider_router.rs:79-96 returns only the current provider and the breaker is never consulted. The real impact needs failover on with >=2 queued providers: the stuck provider is silently skipped (`continue` at :448-450) while others serve, until manual reset (commands/proxy.rs:329) or restart; `NoAvailableProvider` at :1069-1084 needs every candidate stuck. For streaming requests the window is only until upstream headers validate, since `record_success_result` is awaited at :495 before the stream is handed to the client. RAII permit proposal is sound and absent; P2 is proportionate.

### PROXY-04: Blocking SQLite (std Mutex + sync I/O) on the async hot path, 6-10 times per request

**Type:** performance · **Priority:** P2 · **Effort:** L · **Confidence:** medium

- **Evidence:** `src-tauri/src/database/mod.rs:64-70,80` — `Mutex<Connection>` (std), `lock_conn!` blocks; `src-tauri/src/database/dao/proxy.rs:215-222` — `get_proxy_config_for_app` is `async fn` but just takes the std mutex; `:595-603` — `update_provider_health_with_threshold` writes under the same lock. Per request: `src-tauri/src/proxy/handler_context.rs:327-336` (4 reads) + `:362-372` → `src-tauri/src/proxy/provider_router.rs:43-61` (3 reads); per attempt `provider_router.rs:135-159` (1 read + 1 write); per response `src-tauri/src/proxy/response_processor.rs:636-671` (pricing read + log write, inside `tokio::spawn` on a worker thread); `src-tauri/src/proxy/failover_switch.rs:254`; `src-tauri/src/proxy/handlers.rs:83-106` (`std::fs::read_to_string` per `/v1/models`). No `spawn_blocking` anywhere under `src-tauri/src/proxy/`.
- **Problem:** Every request serializes on one SQLite connection and performs fsync-bound writes on tokio worker threads. Under concurrent streaming (sub-agents) this adds latency to first-token and can stall unrelated requests; config reads are redundant (values rarely change).
- **Proposal:** (1) `Arc<RwLock<ProxyRuntimeConfig>>` cache in `ProxyState` (app config, rectifier/optimizer/copilot config, failover queue) invalidated via the existing DB change hook (`database/mod.rs:83`) or explicit `apply_runtime_config`; (2) a dedicated writer task fed by an `mpsc` channel for health updates and usage rows (batched, `spawn_blocking` or a separate connection); (3) move `handle_models` file read to `tokio::fs`.
- **Acceptance:** `grep -c lock_conn` reachable from the request path drops to 0 on the hot path; a load test with 50 concurrent SSE streams shows no p99 regression when a slow disk is simulated; usage rows still appear within 1s.
- **Verification:** **partially-confirmed** — Core facts hold: database/mod.rs:48 imports `std::sync::Mutex`, :64-70 `lock_conn!` blocks, :80 `conn: Mutex<Connection>`; dao/proxy.rs:215-222 `get_proxy_config_for_app` is `async fn` around a sync lock and :595-603 writes under the same lock; no `journal_mode=WAL`/`synchronous` pragma is set (mod.rs:111-116 only foreign_keys/auto_vacuum), so writes are fsync-bound; `rg spawn_blocking src-tauri/src/proxy` is empty. Several cited lines are wrong: the 4 per-request reads are at handler_context.rs:100-108 (not :327-336, which are Gemini path tests) and `select_providers` at :135-137 -> provider_router.rs:43-61; failover_switch.rs has 135 lines, the read is at :83 not :254. A missed mitigation: forwarder.rs:241-273 `record_success_result` runs `record_result` (config read + health write) inside `tokio::spawn` for the common Closed-state success, so the health write is only inline on failures and HalfOpen probes (:1010-1019), and usage logging is already spawned at response_processor.rs:586-602. The reads are point lookups on tiny tables, so the realistic cost is a few ms of mutex contention per write burst on first-token latency for a single-user desktop proxy, not stalls; P2 is proportionate (effort L is right, proposal is sound and not implemented; `ProxyState.config` at server.rs:36 caches only server-level config). handlers.rs:85/:95 sync file reads are confirmed but `/v1/models` is a startup probe, not per request.

### PROXY-05: Native-Anthropic path opens a new TCP+TLS connection per request; the raw-write mechanism is likely redundant

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/hyper_client.rs:396-413` — `TcpStream::connect` + TLS handshake on every call; `:683-687` — a connection driver task is spawned per request; `src-tauri/src/proxy/forwarder.rs:2118-2123` — comment acknowledges reqwest pooling avoids re-handshake but `should_preserve_exact_header_case` (`forwarder.rs:3141-3156`) routes every `anthropic`-format Claude provider to the raw path. `src-tauri/src/proxy/server.rs:5-9,194-197` — server already runs with `preserve_header_case(true)`, which makes hyper store its own `HeaderCaseMap` in request extensions; `hyper_client.rs:294-309` — the fallback path forwards those extensions to a client built with `http1_preserve_header_case(true)` (`:68-72`), and `forwarder.rs:1812-1984` already appends headers in client order.
- **Problem:** Each Claude Code request pays TCP + TLS (plus CONNECT when an upstream proxy is set) and a spawned task. The custom peek/raw-write machinery exists to preserve wire casing, but hyper's built-in extension-based casing (already wired) plus insertion-ordered `HeaderMap` likely yields identical bytes with pooling for free.
- **Proposal:** Add a wire-capture test (fake upstream records raw bytes) comparing raw path vs pooled hyper-util path for a Claude Code request; if equivalent, delete `send_raw_request`/`WriteFilter`/peek and use one pooled client (hyper-util with `HttpsConnector` + proxy connector, or reqwest) for all providers. If order/case truly differs, keep raw writes but add a keyed connection pool of `http1::SendRequest` handles with keep-alive.
- **Acceptance:** Wire-capture test passes; benchmark shows handshake-free second request (TTFB delta ≥ one RTT); `ss -tn` shows connection reuse.
- **Verification:** **partially-confirmed** — Mechanics confirmed: src-tauri/src/proxy/hyper_client.rs:396-403 runs `tokio::net::TcpStream::connect((host, port))` and :406-413 `tls_connector.connect(server_name, stream)` on every call, :683-687 spawns a conn driver per request, no pool exists anywhere (the only `pool` hit is reqwest's `pool_max_idle_per_host` at http_client.rs:220), forwarder.rs:2118-2123 explicitly says reqwest is used for non-Anthropic backends to avoid the raw path re-handshaking, and `should_preserve_exact_header_case` (forwarder.rs:3141-3156) returns true for every Claude provider whose resolved format is `None | Some("anthropic")`. The 'likely identical bytes' claim mostly holds per hyper 1.8.1 source (role.rs:438-458, OrigCaseWriter :576-621): with the server's HeaderCaseMap (server.rs:194-197) forwarded, hyper writes `ordered_headers` in map order with original casing, but names absent from the map (proxy-added `x-api-key`/`authorization`, `content-length`) are Title-Cased because hyper_client.rs:70 sets `http1_title_case_headers(true)`, whereas `build_raw_request` (hyper_client.rs:583-657) emits them lowercase, so equivalence needs that flag turned off; note also that the peek at server.rs:157-176 runs once per connection, so the raw path's order map is stale on keep-alive requests while hyper's HeaderCaseMap is per request, which weakens the case for the raw path further. The proposal underplays real blockers: the pooled fallback has no CONNECT-proxy support and is deliberately skipped when a proxy is set (hyper_client.rs:283-286, 293), it trusts webpki roots only (hyper_client.rs:62-66) while the raw path adds native certs for MITM proxies (:564-579), reqwest cannot preserve casing, and a keyed pool of `http1::SendRequest` cannot coexist with the `WriteFilter` design (:666-700) that discards hyper's own writes, so the 'keep raw writes but pool' fallback is a redesign, not an add-on. Impact is ~2 RTT (TCP + TLS 1.3; rustls default resumption at :574-576 saves crypto, not round trips) plus 1 RTT for CONNECT per request on the main Claude Code path, which is latency only with no correctness or security exposure, so P2 is proportionate rather than P1.

### PROXY-06: No end-to-end tests of the proxy against a mock upstream; `forward()` is untestable

**Type:** test · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/tests/proxy_commands.rs:1-78` — only pricing/multiplier settings commands; `src-tauri/Cargo.toml` — no `wiremock`/`httpmock`; no test starts `ProxyServer` (`grep TcpListener::bind` only hits `server.rs`). `src-tauri/src/proxy/forwarder.rs:1297-1299`, `:1470`, `:1571-1572`, `:1621-1624` — `forward()` reads `app_handle.state::<CopilotAuthState>()`/`CodexOAuthState` (panics if unmanaged) so it cannot run outside Tauri; existing tests construct `RequestForwarder` by hand (`:3492-3516`) and only exercise pure helpers.
- **Problem:** The retry loop (`forwarder.rs:386-1107`), timeouts, 2xx-error-envelope validation, rectifier retries, header rewriting and streaming priming have no executable specification. Regressions in failover semantics are only caught by users.
- **Proposal:** Add `wiremock` (dev-dep) + a `ProxyTestHarness` that builds `ProxyState` with `Database::memory()`, starts `ProxyServer` on `127.0.0.1:0`, and drives requests with reqwest. Abstract the managed-auth lookups behind a `ManagedAuthResolver` trait injected into `RequestForwarder` (test double returns fixed tokens). Seed scenarios: 500→200 failover, 429 Retryable, 400 NonRetryable, SSE mid-stream disconnect, 2xx error envelope, signature rectifier retry, HalfOpen permit release on client abort, header casing passthrough, idle timeout.
- **Acceptance:** `cargo test -p cc-switch --test proxy_e2e` runs ≥10 scenarios in < 30 s with no Tauri runtime.
- **Verification:** **partially-confirmed** — The gap is real: src-tauri/tests/proxy_commands.rs:1-78 only exercises cost-multiplier/pricing hooks, src-tauri/Cargo.toml:112-114 lists only `serial_test` and `tempfile` as dev-deps, `TcpListener::bind` appears only in the production path at server.rs:112 (the coding_plan.rs hits are unrelated std listeners), server.rs and failover_switch.rs have zero tests, and no test anywhere calls `forward_with_retry` (only handlers.rs:196/721/812/927/1764 do), so the retry loop at forwarder.rs:386-1107 and its timeouts have no executable coverage. The 'panics if unmanaged so it cannot run outside Tauri' reasoning is wrong: `app_handle` is `Option<tauri::AppHandle>` (forwarder.rs:125) and every `.state::<CopilotAuthState>()`/`CodexOAuthState` call is guarded by `if let Some(app_handle)` (:1297, :1570, :1621) or `let Some(..) else { return }` (:2467, :2499), `test_forwarder` already builds with `app_handle: None` (:3505), and `ProxyServer::new(config, db, None)` (server.rs:63-67) with `ProxyState.app_handle: Option` (:48) can start without Tauri; the only thing lost is that the Copilot/Codex-OAuth token-injection branches silently skip, which is the real reason a `ManagedAuthResolver` seam is useful. 'No executable specification' is overstated for several listed items: 2xx error-envelope detection (forwarder.rs:4158-4238), streaming priming and body-read failure (:3774-3862), header rewriting (:4115-4157, :3698-3772), rectifiers (thinking_rectifier.rs 32 tests, thinking_budget_rectifier.rs 14), circuit breaker (4) and provider_router (6) all have unit tests, though none drive the HTTP path. One proposal detail is missing: `mod proxy` is private (lib.rs:32) and `server` is `pub(crate)` (proxy/mod.rs), so a `--test proxy_e2e` integration test needs public re-exports or `*_test_hook`-style wrappers like commands/proxy.rs:160-245, or must live in-crate. As a test-coverage gap rather than a live defect in a desktop app, P2 is the proportionate priority.

### PROXY-07: Decompose `forwarder.rs`/`handlers.rs` into a staged request pipeline with pluggable transport

**Type:** refactor · **Priority:** P2 · **Effort:** XL · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:1114-2228` — single `forward()` (≈1100 lines) interleaves base-URL resolution, model mapping (`:1159-1190`), Copilot optimizer (`:1199-1292`), live model resolution, Codex OAuth/Copilot token fetch (`:1567-1675`), header policy (`:1812-2046`), body transforms (`:1415-1519`), transport selection (`:2118-2172`) and response validation (`:2177-2205`); `:386-1107` — retry loop duplicates the "success" bookkeeping block four times (`:492-543`, `:591-646`, `:737-795`, `:903-955`). `src-tauri/src/proxy/handlers.rs:156-254`, `:692-755`, `:779-870`, `:894-985`, `:1723-1798` — five handlers with the same read-body/parse/ctx/forward/process skeleton.
- **Problem:** Every new provider quirk is another `if is_copilot`/`if codex_responses_to_anthropic` branch in one function; ordering bugs (e.g. PROXY-24) and missing feedback paths (PROXY-09) are hard to see; the module cannot be unit-tested or extended (new routing/transport strategies) without touching the whole file.
- **Proposal:** Target structure under `src-tauri/src/proxy/pipeline/`: `RequestPlan { url, method, headers, body: Bytes, streaming, api_format }` produced by an ordered list of `RequestStage` impls (`ModelMapping`, `CopilotOptimizer`, `ThinkingRectifier(mode)`, `MediaSanitizer`, `FormatTransform`, `HeaderPolicy`, `Auth`), each a small struct with its own tests; a `Transport` trait (`PooledHyper`, `Reqwest`) returning `ProxyResponse`; `ResponseValidator`s (`Anthropic2xxEnvelope`, `ResponsesStreamStart`) run inside the attempt; an `AttemptLoop` that owns breaker permits (RAII), timeout policy and bookkeeping via a single `AttemptOutcome` enum. Handlers collapse to `handle_proxy_request::<ClaudeProtocol>(state, req)` with a `Protocol` trait providing parser config, endpoint rewriting and response transform hooks.
- **Acceptance:** `forward()` < 150 lines; no function in `proxy/` > 300 lines; PROXY-06 harness green before and after; each stage has unit tests; adding a header rule touches one file.
- **Verification:** **partially-confirmed** — Evidence holds: `forward()` runs from src-tauri/src/proxy/forwarder.rs:1114 to :2232 (next fn at :2234, ~1118 lines) and interleaves the cited stages (model mapping :1159-1190, Copilot optimizer :1199-1292, body transforms :1415-1519, auth/token fetch :1567-1675, header policy :1812-2046, transport selection :2118-2172, 2xx validation :2177-2205); `forward_with_retry_inner` (:386-1107) repeats the same success block (record_success_result, current_providers insert, status update with spawned `try_switch`, return ForwardResult) near-verbatim at :492-543, :591-646, :737-795 and :903-955. The five handlers at handlers.rs:156-254, :692-755, :779-870, :894-985 and :1723-1798 do share the into_parts/collect/parse/RequestContext::new/create_forwarder/forward_with_retry/take-guard/process_response skeleton. The proposal is off in one respect: it designs a parallel `Protocol`/stage architecture without acknowledging the partial abstractions that already exist and should be extended instead: the `ProviderAdapter` trait (providers/adapter.rs:16-55: base_url, auth headers, build_url, needs_transform, transform_request), `RequestContext` (handler_context.rs:36-120), the per-protocol `UsageParserConfig` (handler_config.rs:24-37) and `process_response` (response_processor.rs:321-327). 'The module cannot be unit-tested' is overstated since forwarder.rs:3460-4769 holds ~90 helper tests, though none exercise `forward()` itself. As an XL maintainability refactor with high regression risk that the finding itself makes dependent on the PROXY-06 harness, P2 is proportionate rather than P1.

### PROXY-08: Timeout policy is split across three layers and the raw path has no connect timeout, so an unreachable provider stalls failover for the full non-streaming timeout

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/hyper_client.rs:399-403` — `TcpStream::connect` with no timeout; `:273-278` — only the overall `timeout` wraps connect+TLS+write+headers; `src-tauri/src/proxy/forwarder.rs:2095-2099` — that timeout is `non_streaming_timeout` or 600 s, used for streaming requests too; `:2137-2150` — the reqwest path instead uses `streaming_first_byte_timeout` for the header wait and `http_client.rs:218-219` adds `connect_timeout(30)`; `:2234-2260` — body read restarts the same clock (worst case 2x); `src-tauri/src/proxy/response_processor.rs:216-224` — applies it a third time (no-op on `Buffered`); `src-tauri/src/proxy/handler_context.rs:429-452` — failover off ⇒ all timeouts zero; default `non_streaming_timeout` 600 (`dao/proxy.rs:262`).
- **Problem:** With failover on and an Anthropic-format provider whose host blackholes SYNs, the loop waits up to 600 s before trying P2 (the reqwest path would give up after 30 s). The matrix of (path × streaming × failover) produces different effective timeouts that nobody can reason about, and "failover off disables timeouts" means a hung upstream hangs the client for 10 min+ (hyper) or 24 h (reqwest streaming, `forwarder.rs:2129`).
- **Proposal:** One `TimeoutPolicy { connect: 10s, tls: 10s, response_headers, first_byte, idle, body_total }` resolved once per request from `AppProxyConfig` and applied inside the transport layer regardless of path. Make "failover off" affect only `max_attempts`, not timeouts (document the change). Expose per-phase timeout errors with distinct `ProxyError` variants so categorize/health see the phase.
- **Acceptance:** Test with a non-routable IP (e.g. 10.255.255.1) as P1 and a mock P2: response arrives within connect timeout + ε. Table of effective timeouts documented and asserted by tests.

### PROXY-09: Streams that fail after the first chunk never feed back into the breaker/health, and idle timeouts just truncate the client stream

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:492-496` — provider success recorded as soon as `prime_streaming_response` (`:2397-2430`) sees one chunk; `src-tauri/src/proxy/response_processor.rs:780-784` — later stream errors only log; `:716-727` — first-byte/idle timeout yields `Err(io::Error)` which makes axum drop the chunked body with no SSE terminal event; `src-tauri/src/proxy/error.rs:65-68` — `StreamIdleTimeout` is dead code; `:2962-2967` (forwarder) comment: mid-stream failures "intentionally do not switch providers".
- **Problem:** A provider that reliably returns 200 + one `message_start` and then dies (common with overloaded gateways) is scored healthy forever; the user sees a truncated response with no error event, and nothing in status/health explains why.
- **Proposal:** Attach a `StreamOutcome` reporter to the passthrough stream (success / upstream error / idle timeout / client abort) that calls `router.record_result` (weighted or only for upstream-caused failures) and updates `last_error`. On timeout/upstream error emit a protocol-correct terminal event (Anthropic `event: error`, OpenAI `data: {"error":...}`, Responses `response.failed`) before closing. Optional: when no productive output has been emitted yet, retry on the next provider (already done for Responses via `validate_responses_stream_start`, generalize it).
- **Acceptance:** Mock upstream that sends `message_start` then hangs: breaker opens after N such streams; client receives a terminal error event; `last_error` populated.

### PROXY-10: Circuit breaker error-rate uses lifetime counters (no sliding window) and defaults disagree

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/circuit_breaker.rs:266-283` — `error_rate = failed/total` where both accumulate since the last `Closed` transition (`:380-387`), never time-bounded; `src-tauri/src/proxy/provider_router.rs:137` — fallback `failure_threshold` is 5 while `CircuitBreakerConfig::default()` is 4 (`circuit_breaker.rs:66`); `:120-123` — `update_config` does not reset counters, so changing `min_requests` mid-run compares against stale totals.
- **Problem:** After 1 000 successes a provider must fail ~1 500 times before the 60 % rule fires, so the rule is effectively only the consecutive-failure threshold; conversely the first 10 requests can open the breaker on a transient blip. Mismatched defaults produce different DB health vs breaker state.
- **Proposal:** Replace counters with a time-bucketed ring (e.g. 10 × 6 s buckets) or last-N ring buffer; compute error rate over the window; align the default and derive it from one constant; reset the window on config change.
- **Acceptance:** Unit tests: 1 000 successes then 6/10 failures opens; 100 successes 60 s ago + 2 failures now does not; defaults equal in both places.

### PROXY-11: A single-provider failover queue causes a 60 s total outage once the breaker opens

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:411-412` — breaker `allow_request` bypassed when `providers.len() == 1`, but `:1010-1019` still records failures; `src-tauri/src/proxy/provider_router.rs:65-78` — selection filters by `is_available()` whenever failover is enabled; `:98-102` — empty result ⇒ `AllProvidersCircuitOpen` → 503 (`error.rs:131-133`) for `timeout_seconds` (default 60).
- **Problem:** With failover enabled and exactly one queued provider (the default created by `set_auto_failover_enabled`, `commands/failover.rs:100-129`), four consecutive 5xx/429s make every request fail locally for a minute even though there is nothing to fail over to; the only "benefit" is protecting an upstream that is already rate limiting.
- **Proposal:** When filtering leaves no candidates, fail open with the candidate whose `last_opened_at` is oldest (and mark the attempt as a probe), or skip breaker gating in selection when the queue has one entry (mirroring the forwarder bypass). Surface breaker state in the UI instead of 503s.
- **Acceptance:** Test: queue of one provider, 4 failures, next request is forwarded (not 503).

### PROXY-12: Thinking-signature rectifier fires on any upstream error containing "invalid request" and strips all thinking context

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/thinking_rectifier.rs:100-106` — scenario 7 matches `"invalid request"`, `"illegal request"`, `"非法请求"` anywhere in the (full JSON) error body; `src-tauri/src/proxy/forwarder.rs:2589-2594` — the whole upstream body is passed; `:671-819` — on match the proxy rewrites the body (removing every `thinking`/`redacted_thinking` block and `signature`, `thinking_rectifier.rs:118-189`) and re-sends to the same provider.
- **Problem:** Unrelated 400s from gateways ("Invalid request: max_tokens exceeds…", "invalid request body") cost a second billed request and silently discard the assistant's prior reasoning, which changes model behavior for the rest of the session. The budget rectifier then may also fire (`:822-978`), so one bad request can become three upstream calls.
- **Proposal:** Require a thinking/signature keyword for scenario 7 (or make it an opt-in "aggressive" mode in `RectifierConfig`); log a `[RECT-*]` reason code with the matched phrase; cap total rectifier retries per request at one.
- **Acceptance:** Unit tests: "Invalid request: max_tokens too large" does not trigger; existing signature cases still do; forwarder test asserts ≤ 2 upstream calls per provider.

### PROXY-13: Full prompts, response bodies and all upstream headers are written to the persistent log file

**Type:** security · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:2084-2092` — entire request body at debug; `src-tauri/src/proxy/response_processor.rs:102-107`, `:154-159` — `format_headers` (`:801-810`) dumps every response header value; `:227-231` — entire non-streaming response body; `:765-767` — every SSE data line; `src-tauri/src/proxy/handlers.rs:545`, `:550`, `:1113`, `:1118`, `:1273` — full upstream bodies at **error** level (active at the default `info` filter); `src-tauri/src/proxy/hyper_client.rs:260-266` — `proxy={:?}` prints the upstream proxy URL including `user:pass@` (while `http_client.rs:319-335` has `mask_url`). Logs go to `cc-switch.log` on disk (`src-tauri/src/lib.rs:350-360`) and the level is user-selectable up to trace (`types.rs:345-378`).
- **Problem:** Users asked to "enable debug logging and attach the log" ship their source code, prompts, upstream `set-cookie`/auth echoes and proxy credentials. Error-level body dumps leak even at default settings.
- **Proposal:** Central `redact_headers()` (authorization, x-api-key, x-goog-api-key, cookie, set-cookie, chatgpt-account-id) used by every header log; cap body excerpts at 512 B via `summarize_text_for_log`; keep full-body capture behind an explicit "diagnostic capture" toggle (see PROXY-21) with a size cap; use `mask_url` for proxy URLs.
- **Acceptance:** grep for `format_headers`/`body_str` in log macros finds only redacted/truncated variants; a test asserts no header value containing "Bearer" appears in captured log output.

### PROXY-14: Server stop does not drain connections, `ProxyServer` has no `Drop`, and `StopTimeout` is treated as fatal by config updates

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/server.rs:141-213` — shutdown only breaks the accept loop; per-connection tasks (`:156-202`) keep running; `:225-255` — `stop()` waits 5 s for the accept task only and returns `Err(StopTimeout)`; no `impl Drop for ProxyServer`; `src-tauri/src/services/proxy.rs:3061-3066` — `update_config` propagates the stop error (so an address change fails although the old server did stop), and on later error paths `server_guard.take()` drops the struct without stopping it.
- **Problem:** In-flight streams continue against a server the UI reports as stopped; a dropped-but-not-stopped `ProxyServer` orphans a listener on the old port (port appears "in use" on restart); config changes can spuriously fail.
- **Proposal:** Track connections with `hyper_util::server::graceful::GracefulShutdown`; `stop()` = close listener → signal graceful → wait bounded drain → report counts; `impl Drop` sends the shutdown signal; make `StopTimeout` a warning in `update_config`.
- **Acceptance:** Test: start, open a long SSE request, call `stop()`: listener closed immediately, stream finishes or is aborted within the drain budget, task count returns to baseline; dropping `ProxyServer` without `stop()` frees the port.

### PROXY-15: No per-connection timeouts or limits on the accept side; `peek` can block a task forever

**Type:** security · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/server.rs:157-175` — `stream.peek` with no timeout before any HTTP parsing; `:194-197` — `http1::Builder::new()` without `.timer(...)`, so hyper's header read timeout is disabled (hyper 1.8.1 per `Cargo.lock`, which needs a timer to enforce it); no connection cap or idle keep-alive timeout anywhere in `server.rs`.
- **Problem:** A client that opens sockets without sending bytes pins tasks and file descriptors indefinitely; combined with PROXY-03 this is a trivial remote DoS; locally, misbehaving tools (browser preconnect) leak tasks.
- **Proposal:** `tokio::time::timeout(5s, peek)`; `.timer(TokioTimer::new()).header_read_timeout(30s)`; keep-alive idle timeout; a `Semaphore` connection cap (e.g. 256) with 503 on overflow; log and drop on violation.
- **Acceptance:** Test opening 10 idle sockets: tasks are reaped within the timeout; a slowloris-style client gets disconnected.

### PROXY-16: `ProviderRouter` holds the breaker-map write lock across a blocking DB call

**Type:** performance · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/provider_router.rs:243-266` — `circuit_breakers.write().await` taken, then `self.db.get_proxy_config_for_app(app_type).await` (std mutex + SQLite query) executed while holding it; `:197-213` — `update_all_configs`/`update_app_configs` hold the read lock across `update_config().await` for every breaker; `:65-73` — `select_providers` calls `get_or_create_circuit_breaker` per candidate per request.
- **Problem:** The first request for any provider (and every config update) blocks all other requests' breaker lookups behind SQLite; combined with PROXY-04 this serializes request admission.
- **Proposal:** Read the config before acquiring the write lock (double-checked insert); store breakers in `dashmap` or `RwLock<HashMap<String, Arc<CircuitBreaker>>>` with critical sections free of `.await`; collect breakers into a `Vec` before updating configs.
- **Acceptance:** Clippy lint `await_holding_lock`-style review passes; no `.await` inside a `circuit_breakers.write()` scope.

### PROXY-17: Every request body is fully buffered, parsed to `serde_json::Value`, cloned per attempt and re-serialized with canonical key order

**Type:** performance · **Priority:** P2 · **Effort:** L · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/handlers.rs:169-175` (and `:701-708`, `:791-798`, `:906-913`, `:1732-1744`) — whole body collected then parsed; `src-tauri/src/proxy/server.rs:368` — 200 MB limit; `src-tauri/src/proxy/forwarder.rs:451-463` — `body.clone()` (full `Value` tree) per provider attempt, `:559` another clone for media retry; `:3358-3360` — `prepare_upstream_request_body` canonicalizes (re-sorts every object) and `:2053` re-serializes; `src-tauri/src/proxy/handler_context.rs:478-480` — `Vec<Provider>` (with `settings_config` JSON) cloned per request; `ForwardResult.provider` cloned again (`forwarder.rs:538`).
- **Problem:** Multi-MB agent contexts are deserialized/cloned/serialized several times per request (CPU + memory spikes proportional to attempts), and the bytes sent upstream are never the client's bytes even in pure passthrough mode (key order rewritten), which complicates byte-level debugging and any future request signing. Malformed JSON yields a 500 (see PROXY-26).
- **Proposal:** Parse once into `Arc<Value>`; make stages return `Cow`/`Option<Value>` so an untouched body is forwarded as the original `Bytes` (passthrough fast path); only canonicalize when a cache-key hash is actually needed (`log_prompt_cache_trace` at debug); share `Arc<Provider>` across the loop. Consider a streaming JSON size guard lower than 200 MB per route.
- **Acceptance:** Benchmark with a 5 MB body: ≤1 parse and ≤1 serialize when no transform applies; wire-capture test shows passthrough bodies are byte-identical to client bodies.

### PROXY-18: Usage logging is re-implemented six times with divergent rules

**Type:** refactor · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/response_processor.rs:459-554` (collector), `:556-603` (`spawn_log_usage`), `:620-674` (`log_usage_internal`); `src-tauri/src/proxy/handlers.rs:286-368` (`ClaudeUsageLog`), `:426-484`, `:1009-1071`, `:1144-1183`, `:1308-1347`, `:1382-1437`, and `:2385-2435` (`log_usage`, a copy of `log_usage_internal`). Rules differ: some paths skip all-zero usage (`handlers.rs:1033-1036`, `:1144-1146`), `response_processor.rs:264-279` writes a zero row; model fallback chains are hand-rolled in each.
- **Problem:** Billing semantics (dedup id, zero-usage rows, model attribution, pricing anchor) drift between Claude/Codex/Gemini/transform paths; fixing one bug requires six edits.
- **Proposal:** `UsageSink` trait with one `record(UsageEvent { ctx_snapshot, usage, model_hint, first_token_ms, streaming, status })`; one `UsageEvent::from_response(parser_config, value)` and `from_stream_events(...)`; collectors become `SseUsageCollector::new(parser_config, sink)`. Delete `handlers::log_usage`.
- **Acceptance:** One implementation of the pricing-anchor/zero-row rule, covered by tests; `rg "fn log_usage"` returns one definition.

### PROXY-19: Routing is a fixed priority list; no sticky sessions, load balancing, per-provider concurrency/rate limits, or retry budget

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/provider_router.rs:51-78` — candidates are the failover queue in sort order, filtered by breaker only; `src-tauri/src/proxy/forwarder.rs:415-431` — attempts bounded only by `max_attempts`; session identity is extracted (`src-tauri/src/proxy/session.rs:237-262`) and threaded into `RequestForwarder` (`forwarder.rs:129-131`) but never used for routing; rectifier/media retries (`forwarder.rs:553-978`) can add up to three extra upstream calls per provider with no global budget.
- **Problem:** Mid-conversation failover breaks prompt caching and thinking-signature validity (every provider has its own cache/signature domain), users with multiple keys cannot spread load, and a flapping upstream can trigger retry storms that burn quota.
- **Proposal:** `RoutingStrategy` trait selected per app: `Priority` (today), `RoundRobin`, `WeightedRandom`, `LeastLatency` (EWMA from usage logs); `StickySession { ttl }` mapping `session_id → provider_id` (bounded LRU) consulted first while the pinned provider is available; per-provider `max_concurrent` (`Semaphore`) and token-bucket `rpm`/`tpm`; a retry budget (e.g. ≤ 10 % of requests in the last minute may retry) enforced in the attempt loop. Persist strategy settings in `AppProxyConfig`.
- **Acceptance:** E2E tests per strategy; sticky test shows the same session stays on P2 after P1 recovers until TTL; retry budget test shows retries suppressed under a storm.

### PROXY-20: No metrics endpoint, tracing spans, or OpenTelemetry export

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/types.rs:58-92` — status is a handful of global counters; `src-tauri/src/proxy/log_codes.rs` — log codes exist but there are no spans/request ids in logs (`log::` only, no `tracing`); `src-tauri/src/proxy/handlers.rs:68-71` — `/status` is the only machine-readable surface and omits uptime/active targets that `ProxyServer::get_status` adds (`server.rs:257-277`); per-provider latency/first-token/error histograms exist only in SQLite usage rows.
- **Problem:** Users and maintainers cannot answer "which provider is slow/failing right now" without reading logs; support threads rely on ad-hoc debug logs (see PROXY-13).
- **Proposal:** Adopt `tracing` with a per-request span (request_id, session_id, app, provider, attempt, phase) bridged to the existing `log` sink; `metrics` crate with `/metrics` (Prometheus text) exposing per-provider request/latency/ttfb histograms, breaker state gauge, active connections, retries, duplicate-send risk; optional OTLP exporter behind a setting; make `/status` and the UI dashboard read from the same registry.
- **Acceptance:** `curl /metrics` lists per-provider histograms; one request produces one span tree with attempt children; UI "proxy stats" sourced from the registry.

### PROXY-21: No request flight recorder, replay, or export for debugging failed requests

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/handlers.rs:2351-2378` — failures only produce a `log_error_with_context` row (status + message); request/response bodies live only in debug logs (`forwarder.rs:2084-2092`, `response_processor.rs:227-231`); no Tauri command exposes recent requests (`src-tauri/src/commands/proxy.rs` has none).
- **Problem:** Reproducing provider-specific transform bugs (the bulk of issues referenced in comments: #2234, #5025, #666 …) requires users to hand-collect logs; maintainers cannot replay the exact outbound request against a provider.
- **Proposal:** In-memory ring buffer (e.g. last 200 requests, redacted per PROXY-13, bodies truncated to a configurable cap) with the final outbound `RequestPlan`, upstream status/headers, first N KB of response, attempt list and timings; Tauri commands `list_recent_requests`, `get_request_detail`, `replay_request(id, provider_id)`, `export_requests_jsonl`; UI panel. Opt-in "capture full bodies" toggle.
- **Acceptance:** After a failing request the UI shows the attempt timeline and outbound request; replay re-sends it and shows the diff of responses; export produces JSONL without credentials.

### PROXY-22: No active health probing; breaker recovery always burns a real user request (and `health.rs` is a stub)

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/health.rs:1-7` — `HealthChecker` placeholder only; `src-tauri/src/proxy/circuit_breaker.rs:133-154` — Open→HalfOpen happens only when a live request arrives; the probe is the user's request (`forwarder.rs:435-447`); README.md:228 advertises "provider health monitoring"; existing `services/stream_check.rs` is UI-triggered, not periodic.
- **Problem:** After an outage the first user request to each recovered provider is sacrificed as a probe (with max 1 concurrent probe, other users get `NoAvailableProvider`), and a provider that recovered minutes ago is not used until traffic happens to hit it; the UI cannot show "recovering".
- **Proposal:** Background `HealthChecker` task per app: when a breaker is Open and its timeout elapsed, send a cheap probe (`GET /v1/models` or a `max_tokens: 1` request, configurable, rate-limited) through the same pipeline with a `probe=true` flag that bypasses usage logging; on success close the breaker and trigger the existing "switch back to higher priority" logic (`commands/proxy.rs:347-406`) automatically.
- **Acceptance:** Mock provider: fail → open → recover; breaker closes within `timeout_seconds + probe_interval` with zero user requests; UI shows probe results.

### PROXY-35: No developer or user documentation of the proxy pipeline, failover semantics, timeout rules, or header policy

**Type:** docs · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `docs/user-manual/en/4-proxy` exists but repo-level docs do not describe: retryable status buckets (`src-tauri/src/proxy/forwarder.rs:2559-2574`), "failover off ⇒ no timeouts" (`handler_context.rs:429-452`), which client headers are silently dropped (`forwarder.rs:1834-1865`, including `traceparent`/`x-request-id`, which breaks users' own tracing), rectifier trigger phrases (`thinking_rectifier.rs:45-106`), 2xx-error-envelope failover (`forwarder.rs:2262-2313`), or the `0.0.0.0` exposure (PROXY-03). README.md:228 promises "provider health monitoring" that is a stub (`health.rs`).
- **Problem:** Behavior that users depend on (and that support must explain) lives only in Chinese code comments; contributors cannot change the forwarder safely.
- **Proposal:** `docs/dev/proxy-architecture.md` (request lifecycle sequence diagram, breaker state machine, timeout matrix per path/streaming/failover, header policy table, rectifier catalog with examples, error codes); user-manual section on network exposure and failover semantics; keep it updated via a CI check that lists header policy constants.
- **Acceptance:** Docs merged and linked from README; a new contributor can locate where a header is dropped from the doc alone.

### PROXY-37: Underscore-key filter deletes legitimate keys inside tool_use inputs, tool results, Gemini function args and metadata on every upstream body

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:3358-3360` — `prepare_upstream_request_body` runs `filter_private_params_with_whitelist(body, &[])` (empty whitelist) on every outbound body, at `:1527` and again at `:1535`; `src-tauri/src/proxy/body_filter.rs:82-101` — every object key starting with `_` is dropped recursively, the only exemption being direct children of `properties`/`patternProperties`/`definitions`/`$defs` (`:120-125`); no exemption exists for `messages[].content[].input` (Anthropic `tool_use`), structured `tool_result` content, Gemini `functionCall.args`/`functionResponse.response`, OpenAI Responses `input[]` items, or client `metadata`. The only test touching underscores (`body_filter.rs:301-325`) proves schema *names* survive, not replayed call *values*.
- **Problem:** A tool whose schema legitimately declares `_id`/`_meta` (MCP's `_meta` is part of the MCP spec; `_id` is ubiquitous in JSON APIs) keeps the key in `tools[].input_schema`, but when the model calls it with `{"_id": ...}` the replayed assistant `tool_use.input` on the next turn silently loses the key. The history the upstream sees no longer matches what the model emitted and what the tool actually received; schemas with `required: ["_id"]` make the replayed call invalid on strict gateways; Gemini `functionCall.args` suffers identically. The module's stated purpose (strip proxy-private params like `_internal_id`) is defeated by applying it to opaque model/user-owned payloads.
- **Proposal:** Restrict the filter to the request's own top-level keys (optionally plus `metadata`), never descending into `messages`, `input`, `contents`, `tools` or `system`; or invert the design and strip only keys the proxy itself adds. Add round-trip regression tests with `tool_use.input._meta` (Anthropic), `input[].arguments` containing `_id` (Responses) and `functionCall.args._id` (Gemini).
- **Acceptance:** A request whose history contains `tool_use.input._meta` / `functionCall.args._id` is forwarded with those keys intact; a top-level `_debug` key is still removed; new unit tests cover both cases for all three body formats.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### PROXY-38: Prefixed client routes /claude/* and /gemini/* are forwarded to the upstream with the prefix intact (404 on passthrough)

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/server.rs:297-298` registers `/claude/v1/messages` and `:363-366` registers `/gemini/v1beta/*path` and `/gemini/v1/*path` as prefixed aliases; `src-tauri/src/proxy/handlers.rs:180-186` strips a prefix only when `strip_prefix` is `Some`, which is passed only for `/claude-desktop` (`:135`), while the Claude route passes `None` (`:121`); `handlers.rs:1752-1755` forwards the full `path_and_query` for Gemini; `src-tauri/src/proxy/providers/claude.rs:786-811` and `providers/gemini.rs:213-227` `build_url` simply concatenate base + endpoint (only de-duplicating `/v1/v1` / `/v1beta/v1beta`); no `strip_prefix("/claude")` or `"/gemini"` exists anywhere under `src-tauri/src`. Only the Claude *transform* path escapes because `rewrite_claude_transform_endpoint` recognizes `/claude/v1/messages` (`forwarder.rs:2732-2734`, `:2997`). Codex, by contrast, normalizes the endpoint (`handlers.rs:712`, `:802`).
- **Problem:** A client configured with `http://127.0.0.1:15721/claude` against a native Anthropic provider, or `http://127.0.0.1:15721/gemini`, is forwarded to `https://api.anthropic.com/claude/v1/messages` / `https://generativelanguage.googleapis.com/gemini/v1beta/models/...:generateContent` and receives an upstream 404. Because 404 is Retryable (`forwarder.rs:2571-2574`) the proxy then walks every provider in the failover queue, consuming HalfOpen permits and health scores, before surfacing the 404. The routes advertise support for prefixed URLs but never worked for passthrough.
- **Proposal:** Normalize the inbound path in the handlers: pass `Some("/claude")` / `Some("/gemini")` as `strip_prefix` for the prefixed routes (or derive the endpoint from the axum `*path` capture), mirroring the `/claude-desktop` handling, and add route-level tests asserting the upstream URL built for every registered route.
- **Acceptance:** A request to `/gemini/v1beta/models/gemini-2.5-pro:generateContent` reaches `<base>/v1beta/models/gemini-2.5-pro:generateContent`; `/claude/v1/messages` with an `anthropic` api_format provider reaches `<base>/v1/messages`; unprefixed routes are unchanged; tests cover each registered prefixed route.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### PROXY-39: Emptying the failover queue (or deleting its last provider) while failover is enabled turns every request into 503 'no providers configured'

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/provider_router.rs:51-78` — with `auto_failover_enabled` the candidate set is *only* the failover queue; `:98-105` — an empty result with `total_providers == 0` returns `AppError::NoProvidersConfigured`, mapped to 503 `未配置供应商` (`error.rs:35-36`, `:134-136`); `src-tauri/src/commands/failover.rs:49-59` — `remove_from_failover_queue` has no guard, while the enable path checks emptiness only at enable time (`:100-134`); `src/components/proxy/FailoverQueueManager.tsx:91-105` lets the user remove any entry. Queue membership is a column on the provider row, so deleting the last queued provider has the same effect.
- **Problem:** The user still has a valid current provider and an active takeover, yet Claude Code/Codex get 503 `未配置供应商` on every request until they realise the queue is empty; the message blames configuration that is present, nothing disables failover or emits an event, and the header toggle still shows failover as enabled.
- **Proposal:** In `select_providers`, when failover is enabled but the queue yields no candidates, fall back to the effective current provider (the failover-off branch) and log a warning; in addition refuse (command + UI) to remove the last queue entry while failover is on, or auto-disable failover with a toast/event when the queue becomes empty (including via provider deletion).
- **Acceptance:** With failover on and an emptied queue, requests are still served by the current provider and `last_error` carries a warning; a `provider_router.rs` unit test covers 'failover on + empty queue'; the UI either blocks removing the last entry or shows the auto-disable toast.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### PROXY-40: Trust store differs per HTTP stack: corporate/MITM CAs work for native Claude (raw path) but not for Codex, Gemini, Copilot or the fallback client; no custom-CA option

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/Cargo.toml:42` — `reqwest` is built with `rustls-tls` (webpki roots only; `rustls-tls-native-roots` is not enabled) and `src-tauri/src/proxy/http_client.rs:216-264` never calls `use_preconfigured_tls`/`add_root_certificate`; `src-tauri/src/proxy/hyper_client.rs:59-73` — the pooled fallback client uses `with_webpki_roots()` only; `:564-579` — the raw path loads webpki **plus** `rustls_native_certs` precisely so 'proxy MITM CAs (Clash, mitmproxy)' are trusted; `src-tauri/src/proxy/forwarder.rs:2110-2172` — the stack is chosen by `should_preserve_exact_header_case` (`:3141-3156`): only native-Anthropic providers get the raw path, everything else (Codex, Gemini, Copilot, Codex OAuth, every transform format, any SOCKS proxy) goes through reqwest. PERF-26 in the roadmap notes the two hyper stores but proposes sharing a config between the two hyper paths only, which does not touch reqwest.
- **Problem:** On a corporate network or behind a TLS-intercepting proxy, Claude requests succeed while every Codex/Gemini/Copilot request fails with `invalid peer certificate: UnknownIssuer`; the same Claude provider flips to failing as soon as its api_format changes or a SOCKS proxy is configured. Users cannot add a CA or opt out of verification per provider, so the only workaround is disabling the interceptor, and the behaviour looks like a provider outage.
- **Proposal:** Build one `rustls::ClientConfig` (webpki + native certs, plus an optional user-supplied PEM from settings) and use it for reqwest (`Client::builder().use_preconfigured_tls(config)` or `rustls-tls-native-roots` + `add_root_certificate`), the hyper-util client and the raw connector; expose an 'extra CA certificate' setting (and an explicit, off-by-default per-provider 'skip TLS verification' with a warning) in proxy settings.
- **Acceptance:** With a self-signed CA installed in the OS store, requests through Codex/Gemini/Copilot providers succeed exactly like native Claude; a test asserts all three client paths derive from the same root-store builder; the CA setting is documented.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### PROXY-41: Native Anthropic/Gemini/Chat streaming paths have no semantic stream-start validation; a 2xx stream whose first event is an error is scored as success and never fails over

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:2177-2205` — after `prepare_success_response_for_failover`, semantic validation runs only for `codex_responses_to_anthropic` (`:2185-2188`) and `openai_responses` (`:2189-2204`, via `validate_responses_stream_start` `:2315-2395`, which keeps priming until a productive event or a terminal failure); `:2397-2430` — every other streaming response is committed after *any* first chunk; `:492-496` — provider success is recorded at that point. Anthropic-compatible gateways emit `event: error` / `{"type":"error","error":{"type":"overloaded_error"}}` with HTTP 200 as the first SSE block; Gemini relays emit `{"error":{...}}` as the first chunk.
- **Problem:** For the primary use case (native Anthropic providers) a relay that answers 200 followed immediately by an `overloaded_error`/`rate_limit_error` event resets the breaker's failure counter, bumps `success_requests`, and the user receives the error with no failover even though other queue providers are healthy. The Responses path already solves exactly this; behaviour is inconsistent across formats.
- **Proposal:** Generalise `validate_responses_stream_start` into a per-format `inspect_stream_start(block) -> None | Ok | Err` (Anthropic: `error` event or `type: error` → Err, `message_start`/`content_block_*` → Ok; Gemini: top-level `error` → Err, `candidates` → Ok; Chat: `error` with a reportable message → Err, `choices` → Ok) and run it inside the retry loop for all streaming responses with the same 256 KB cap; map Err to `TransformError` so it stays Retryable.
- **Acceptance:** A mock upstream returning 200 + `event: error` as the first Anthropic SSE block triggers failover to the next provider and records a failure for the first; a normal `message_start` stream is replayed byte-identically; tests exist for Anthropic, Gemini and Chat formats.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### PROXY-23: SSE usage collectors buffer every "usage"-matching event for the whole stream, and the passthrough SSE buffer is unbounded

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/handler_config.rs:46-52` — OpenAI/Codex filters match any chunk containing `"usage"` (OpenAI streams with `include_usage` carry `"usage":null` on every delta); `src-tauri/src/proxy/response_processor.rs:398-402` — each match is JSON-parsed and pushed into `Vec<Value>` kept until stream end (`:410-421`); `:686`, `:743-746` — the `buffer: String` only drains on a blank line, so a non-SSE or single-giant-event body grows without bound (the priming path caps at 256 KB, `forwarder.rs:2319`, but passthrough does not).
- **Problem:** Long agent streams hold O(n) parsed JSON per stream and spend CPU parsing every delta; a misbehaving upstream labelled `text/event-stream` can make the proxy hold the whole response in memory.
- **Proposal:** Replace the `Vec<Value>` with an incremental reducer per protocol (keep only last usage/model/message_id/stop reason); tighten the filters (`"usage":{`); cap the SSE parse buffer (e.g. 1 MB) and switch to passthrough-only mode when exceeded.
- **Acceptance:** Memory test: a 10 k-chunk OpenAI stream keeps collector memory constant; buffer cap test passes.

### PROXY-24: Rectified request body is discarded when failing over to the next provider, doubling calls on every subsequent provider

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:416-418` — comment claims the next provider "仍能用整流后的请求体"; `:451-463` — `provider_body` is rebuilt from `body.clone()` at the top of every loop iteration, so signature/budget/media rectifications (`:703`, `:854`, `:559-563`) are lost; `handle_rectifier_retry_failure` (`:282-337`) `continue`s to the next provider.
- **Problem:** When P1 rejects thinking signatures and then times out on the rectified retry, P2 receives the original (invalid) body, fails, is rectified and retried again: two billed calls per provider instead of one.
- **Proposal:** Keep an `Option<Value>` "last rectified body" outside the loop and start the next provider from it when the rectification is provider-independent (signature/budget/media), or carry `RectifierState` across attempts in the pipeline (PROXY-07).
- **Acceptance:** E2E: P1 → signature error then 500; P2 receives a request with no thinking blocks on its first call.

### PROXY-25: Request-side hop-by-hop headers (`connection`, `keep-alive`, `te`, `upgrade`, `expect`) are forwarded upstream

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:1834-1865` — the drop list contains only `content-length`, `transfer-encoding`, forwarding and tracing headers; `src-tauri/src/proxy/response_processor.rs:35-68` — hop-by-hop stripping exists only for responses; `src-tauri/src/proxy/hyper_client.rs:582-659` — raw writer emits whatever is in the map and sends the body immediately.
- **Problem:** A client `Connection: close` kills upstream keep-alive on the reqwest path; `Expect: 100-continue` is forwarded while the body is already sent (protocol violation some gateways reject); `Connection`-listed custom headers leak.
- **Proposal:** Apply RFC 7230 §6.1 stripping (fixed list + `Connection`-listed names) to requests in the header policy stage, mirroring `strip_hop_by_hop_response_headers`.
- **Acceptance:** Unit test on the header policy; wire-capture test shows no `Connection`/`Expect` from the client reaches the upstream.

### PROXY-26: Malformed client JSON yields HTTP 500 and is not logged as a client error

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/handlers.rs:174-175`, `:707-708`, `:797-798`, `:912-913`, `:1742-1743` — parse errors map to `ProxyError::Internal` → 500 (`error.rs:156-158`); `log_forward_error` is not called on this path; unsupported request encodings correctly use `InvalidRequest` (`handlers.rs:656-660`).
- **Problem:** Clients and the error-rate statistics see a proxy server error for a client bug; support triage misattributes it to the proxy.
- **Proposal:** Map to `ProxyError::InvalidRequest` (400) with the serde error position; keep `Internal` for body read failures.
- **Acceptance:** Test: `POST /v1/messages` with `{` returns 400 with `type: proxy_error, code: cc_switch_invalid_request`.

### PROXY-27: `failover_count` counts "served provider != local current provider", not actual failovers, and switch tasks are spawned per request

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:512-527` (and `:615-631`, `:761-779`, `:925-940`) — `should_switch` compares with `current_provider_id_at_start` (device setting read in `handler_context.rs:338-339`); when true it increments `failover_count` and spawns `try_switch` even if this was the first attempt; `:473-476` — `status.current_provider` is overwritten per attempt; `src-tauri/src/proxy/failover_switch.rs:221-243` — dedup only while a switch is in flight, so N concurrent first requests run the switch (live-config rewrite, tray rebuild) up to N times sequentially.
- **Problem:** UI "failover count" is wrong whenever the queue head differs from the manually selected provider; bursts of parallel sub-agent requests trigger repeated live-config writes.
- **Proposal:** Count failovers when `attempted_providers > 1` (or any failed attempt preceded success); compare against the proxy's own `current_providers[app]` and only switch when the served provider changed since the last served one; remember the last switched target in `FailoverSwitchManager` to make repeats no-ops.
- **Acceptance:** Test: queue [B, A], current A, 5 parallel requests served by B → `failover_count == 0`, exactly one switch executed.

### PROXY-28: Dead/stub modules and duplicated error→status mapping

**Type:** tech-debt · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/health.rs:1-7` (stub); `src-tauri/src/proxy/response_handler.rs:1-232` — `StreamHandler`/`NonStreamHandler`/`ResponseDispatcher` fully `#[allow(dead_code)]`, duplicating `response_processor`; `src-tauri/src/proxy/session.rs:107-185` — `ProxySession` dead; `src-tauri/src/proxy/error.rs:189-206` — `categorize_error` dead; `src-tauri/src/proxy/handler_config.rs:181-228` — `HandlerConfig` consts dead; `src-tauri/src/proxy/error.rs:79-175` vs `src-tauri/src/proxy/error_mapper.rs:225-270` — two status tables kept in sync by a test; `src-tauri/src/commands/proxy.rs:446-455` — `get_circuit_breaker_stats` always returns `None` although `ProviderRouter::get_circuit_breaker_stats` exists (`provider_router.rs:217-230`); `src-tauri/src/proxy/thinking_rectifier.rs:239-242` — `normalize_thinking_type` is an identity function still called in `forwarder.rs:1169`.
- **Problem:** Readers (and this review) must first determine which of two implementations is live; the UI cannot show breaker stats although the data exists.
- **Proposal:** Delete dead modules/functions; derive `IntoResponse` from `map_proxy_error_to_status`; wire `get_circuit_breaker_stats` through `ProxyService` → `ProviderRouter`; implement or remove `HealthChecker` (see PROXY-22).
- **Acceptance:** `cargo clippy -- -W dead_code` clean without `#[allow(dead_code)]` in `proxy/`; breaker stats visible in the UI.

### PROXY-29: `ProxyConfig` carries vestigial timeout/retry fields that the forwarder never reads

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/types.rs:4-28` — `max_retries`, `request_timeout` ("已废弃"), `streaming_first_byte_timeout`, `streaming_idle_timeout`, `non_streaming_timeout` on the global `ProxyConfig`; `src-tauri/src/proxy/handler_context.rs:429-452` and `:494-508` — effective values come from per-app `AppProxyConfig`; `src-tauri/src/proxy/response_processor.rs:465-470`, `:605-611` — `state.config` is consulted only for `enable_logging`; `src-tauri/src/proxy/server.rs:372-375` — `apply_runtime_config` replaces the whole struct for that one field.
- **Problem:** Two config structs with overlapping names invite the wrong one being edited (the UI's global timeout fields, if any, would be silently ignored), and migration code keeps the dead columns alive (`database/schema.rs:749`).
- **Proposal:** Reduce `ProxyConfig` to listen address/port/enable_logging (or fold into `GlobalProxyConfig`); drop dead columns in a migration; make `RequestContext` read a cached `AppProxyConfig` (PROXY-04).
- **Acceptance:** `ProxyConfig` has no timeout/retry fields; frontend types updated; migration test passes.

### PROXY-30: Takeover status and the stop-guard ignore Claude Desktop and Hermes routes

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/types.rs:111-119` — `ProxyTakeoverStatus` has no `claude_desktop`/`hermes`; `src-tauri/src/services/proxy.rs:686-724` — only claude/codex/gemini/grokbuild are read; `src-tauri/src/commands/proxy.rs:21-36` — `stop_proxy_server` refuses to stop only for those four; `src-tauri/src/proxy/server.rs:299-307` — `/claude-desktop/*` routes exist and `claude_desktop_config.rs:926` points Claude Desktop at the proxy origin.
- **Problem:** A user can stop the proxy while Claude Desktop (and any Hermes/OpenCode/OpenClaw config routed via `CodexAdapter`, `providers/mod.rs:248`) still points at it, with no warning; the UI status is incomplete.
- **Proposal:** Derive the status from `AppType::all()` and `proxy_config.enabled` rows instead of hard-coded fields; extend the guard; add a unit test enumerating app types.
- **Acceptance:** Enabling Claude Desktop takeover then calling `stop_proxy_server` returns the "still taken over" error.

### PROXY-31: Error messages returned to API clients are Chinese and not machine-readable (outside the Codex path)

**Type:** docs · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/error.rs:11-77` — `thiserror` Display strings are Chinese; `:162-167` — they are returned verbatim in `{"error":{"message":...,"type":"proxy_error"}}` for Claude/Gemini routes; only Codex routes get English, coded errors (`src-tauri/src/proxy/handlers.rs:1538-1701`); `src-tauri/src/proxy/response_processor.rs:723-726` — stream timeout errors are also Chinese.
- **Problem:** Claude Code users outside China see garbled/unknown text in-terminal and cannot search for the error; automated clients cannot branch on a code.
- **Proposal:** Give every `ProxyError` a stable English `code` (reuse `codex_proxy_error_code`) and English message; move localization to the UI layer (AppError::Localized already exists); keep provider-supplied bodies verbatim.
- **Acceptance:** All `ProxyError` Display strings ASCII; `/v1/messages` error body carries `code`; docs list the codes.

### PROXY-32: Budget rectifier rewrites `max_tokens` to a hard-coded 64000 and budget to 32000 regardless of model limits

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/thinking_budget_rectifier.rs:10-16` — constants; `:106-114` — unconditional `budget_tokens = 32000` and `max_tokens = 64000` when below 32001; triggered from `forwarder.rs:822-978` for any "budget_tokens … ≥ 1024" error.
- **Problem:** Models/gateways with an 8 k–32 k output ceiling return a non-retryable 400 on the retry (wasted call), and a client that asked for a 1 024-token budget is silently upgraded to 32 k thinking tokens (cost/latency).
- **Proposal:** Set `budget_tokens = max(1024, requested)` and `max_tokens = max(existing, budget + 1)` clamped by a per-provider `max_output_tokens` when configured (`ProviderMeta.max_output_tokens` already exists, `forwarder.rs:1456-1463`).
- **Acceptance:** Unit tests for the clamp; retry request never exceeds provider ceiling.

### PROXY-33: Upstream-proxy credentials are logged unmasked

**Type:** security · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/hyper_client.rs:260-266` — `proxy={:?}` with the raw `proxy_url` (may contain `user:pass@`); `src-tauri/src/proxy/http_client.rs:319-335` — `mask_url` exists and is used elsewhere in that file but not here.
- **Problem:** Debug logs persist proxy passwords (see PROXY-13).
- **Proposal:** Log `proxy_url.map(mask_url)`.
- **Acceptance:** grep shows no raw `proxy_url` in log macros.

### PROXY-34: `ProxyStatus` counters mutated under an async RwLock in 13 places with `success_rate` recomputed by hand each time

**Type:** tech-debt · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:326-332`, `:357-361`, `:472-476`, `:508-534`, `:611-637`, `:688-695`, `:758-786`, `:840-847`, `:866-873`, `:921-946`, `:988-995`, `:1021-1025`, `:1050-1058`, `:1072-1079`, `:1088-1095`; `:87-113` — `ActiveConnectionGuard` spawns a task just to decrement a counter.
- **Problem:** Write-lock contention on every attempt, inconsistent updates (some paths bump `failed_requests` before/after `last_error`), and a maintenance hazard when adding a counter.
- **Proposal:** `ProxyMetrics` with atomics (`AtomicU64`) and `record_attempt(outcome)`; derive `success_rate` on read; `ActiveConnectionGuard` becomes `fetch_sub` in `Drop` (no spawn). Feeds PROXY-20.
- **Acceptance:** `status.write()` no longer appears in `forwarder.rs`; counters covered by a unit test of `record_attempt`.

### PROXY-36: Circuit breaker and router unit tests miss error-rate, concurrency, hot-update and leak scenarios

**Type:** test · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/circuit_breaker.rs:401-495` — four tests (closed→open by consecutive failures, half-open→closed, permit not reset, reset); no test for the error-rate path (`:266-283`), `update_config` (`:120-123`), concurrent `record_*` from multiple tasks, or permit release on cancellation; `src-tauri/src/proxy/provider_router.rs:329-522` — selection tests only; `src-tauri/src/proxy/response_processor.rs:812-1197` — no test for `create_logged_passthrough_stream` timeouts or `SseUsageFinishGuard` on drop.
- **Problem:** PROXY-02, PROXY-09 and PROXY-10 are exactly the untested paths.
- **Proposal:** Add tests: error-rate open; window reset on close; hot update of `timeout_seconds`; 100 concurrent `record_failure` tasks keep counters consistent; permit released when the awaiting future is dropped; passthrough stream idle timeout yields a terminal event and triggers the finish guard; `SseUsageFinishGuard` fires `finish()` on drop.
- **Acceptance:** Tests exist and pass; mutation of the error-rate branch fails a test.

### PROXY-42: Rectifier-retry failures use a different retryability rule than the main path: 429/401/403/404/408 and auth/config/transform errors abort failover and skip breaker accounting

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:294-298` — `handle_rectifier_retry_failure` treats only `Timeout`, `ForwardFailed` and `UpstreamError{status >= 500}` as provider errors; everything else returns `Some(ForwardError)` after `release_permit_neutral` (`:323-336`), i.e. no failover and no `record_result`; `:2554-2584` — `categorize_proxy_error` for the very same request classifies 429, 401, 403, 404, 408, 409, 451, `ConfigError`, `TransformError`, `AuthError` and `StreamIdleTimeout` as Retryable. The media retry (`:647-666`), signature retry (`:796-815`) and budget retry (`:956-975`) all route through the stricter helper.
- **Problem:** Once any rectifier has fired on provider A, a subsequent 429 (rate limit) or 401 (bad key) from A ends the request immediately instead of trying provider B, and A's health/breaker is not updated, so the next request hits the same wall. The comment at `:292-293` ('client error, next provider cannot fix it') does not hold for rate limits or credential failures, which are provider-specific by definition.
- **Proposal:** Reuse `categorize_proxy_error(&retry_err, provider)` inside `handle_rectifier_retry_failure` (Retryable → record failure + continue; NonRetryable → neutral release + return) and delete the ad-hoc `is_provider_error` match; add a test where the signature rectifier fires, the retry returns 429, and the next provider is attempted with A's failure recorded.
- **Acceptance:** `forward_with_retry` with providers [A, B], where A returns `invalid signature` then 429 and B returns 200, yields B's response; breaker stats for A show one recorded failure; existing rectifier tests still pass.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### PROXY-43: Header-case/order capture is per TCP connection, so every keep-alive request after the first is written with stale header casing and order

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/server.rs:156-175` — `stream.peek` + `OriginalHeaderCases::from_raw_bytes` run once per accepted connection, before the first request; `:178-192` — the same `original_cases` is cloned into every `service_fn` invocation on that connection; `src-tauri/src/proxy/hyper_client.rs:618-645` — `build_raw_request` emits headers in the captured order and appends any header absent from the capture in lowercase at the tail (`:634-645`). `peek` returns only bytes already in the socket buffer (`server.rs:160-163`), so a request whose headers arrive in a second segment is captured partially. Claude Code and Codex use HTTP keep-alive, so most requests are not the first on their connection.
- **Problem:** The raw TCP/TLS path exists solely to reproduce exact wire casing/order for gateways that fingerprint headers. For the 2nd+ request on a connection the order is the first request's, and any header that was not in the first request (for example `anthropic-beta` appearing later, or a changed `x-app`) is emitted lowercase at the end — exactly the signature the feature tries to avoid. The degradation is silent (debug log only).
- **Proposal:** Capture casing per request instead of per connection: read hyper's `HeaderCaseMap` from `req.extensions()` (available with `http1::Builder::preserve_header_case(true)`), or parse the request head per request with a small framed reader in front of hyper; if neither is feasible, discard the connection-level map after the first request, fall back to title-case for subsequent ones, and log at warn.
- **Acceptance:** A test sending two requests with different header sets on one keep-alive connection asserts that the upstream wire bytes of the second request keep its own casing and order, with no lowercase tail headers.
- **Verification:** added by completeness-critic pass (not independently re-verified)
- **Possible overlap with:** SEC-25 (check before scheduling)

### PROXY-44: Codex OAuth session headers are rewritten with the proxy's codex_-prefixed session id, replacing the client's real session_id value

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/session.rs:289-317` — Codex session ids extracted from the `session_id`/`x-session-id` header or `metadata.session_id` are returned as `format!("codex_{session_id}")` (`:295`, `:312`); `src-tauri/src/proxy/handler_context.rs:121-122`, `:236` — that value becomes `RequestContext.session_id` → `RequestForwarder.session_id`; `src-tauri/src/proxy/forwarder.rs:1684-1689` and `:3082-3102` build `session_id`, `x-client-request-id` and `x-codex-window-id` from it, and `:2044-2046` `insert`s them, overwriting the client's original headers; the comment at `:2042-2043` states the intent is to send the client-provided session id. The unit test at `:3864-3883` feeds an unprefixed id, so the prefix is never exercised; the same prefixed value is reused as `prompt_cache_key` on the Chat bridge (`:1437-1443`).
- **Problem:** The ChatGPT Codex backend receives `session_id: codex_<uuid>` and `x-codex-window-id: codex_<uuid>:0` instead of the client's `<uuid>`. Affinity still works because the value is stable, but it diverges from what the official CLI sends, breaks server-side correlation with the user's local Codex session logs, and overwrites a header the client already sent correctly.
- **Proposal:** Keep the `codex_` prefix for internal keys (shadow stores, usage logs) but carry the raw client value separately (e.g. `SessionIdResult.raw_client_id`) and use the raw value for upstream headers and `prompt_cache_key`; when the client already sent `session_id`, pass it through instead of overwriting. Extend the test to go through `extract_session_id`.
- **Acceptance:** With `session_id: 123e4567-...` from the client, the upstream request carries exactly that value in `session_id` and `x-client-request-id` and `<id>:0` in `x-codex-window-id`; internal usage logs still show `codex_<id>`.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### PROXY-45: Upstream response bodies are buffered and decompressed without any size cap; a gzip/zstd bomb from a hostile or buggy relay can exhaust the desktop app's memory

**Type:** security · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/hyper_client.rs:161-184` — `ProxyResponse::bytes()` collects the whole body (`collect()`, `r.bytes()`, or a `BytesMut` loop) with no limit; `src-tauri/src/proxy/response_processor.rs:82-133`, `src-tauri/src/proxy/forwarder.rs:2211-2221`, `:2271-2279`, `:2296-2304` — bodies are then decoded via `decompress_body`; `src-tauri/src/proxy/content_encoding.rs:33-65` — gzip/deflate `read_to_end`, `BrotliDecompress` and `zstd::decode_all` write into unbounded `Vec`s. The request side is capped at 200 MB (`server.rs:368`) but the response side has no counterpart, although a cap pattern already exists (`forwarder.rs:2319`, 256 KB priming cap).
- **Problem:** Providers are arbitrary third-party relays added by the user (or via deep link). A 1 MB zstd/brotli body that inflates to many GB, or a non-streaming 200 that never ends, grows memory without bound inside the Tauri process that also hosts the UI, so one bad relay response can crash the whole app; Cloudflare HTML error pages and similar are likewise read in full.
- **Proposal:** Add a configurable `max_response_bytes` (default e.g. 64 MB) enforced in `ProxyResponse::bytes()` (via `http_body_util::Limited` or a counting loop) and a decompressed-size limit in `decompress_body` (`Read::take` on each decoder); surface violations as `TransformError`/`UpstreamError` so failover still applies.
- **Acceptance:** A mock upstream returning a 2 MB gzip body that inflates to 2 GB is rejected with bounded allocation (RSS stays below limit + slack) and the request fails over; normal responses are unaffected; unit tests cover both caps.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### PROXY-46: mask_url byte-slices a non-URL string at index 20 and panics on multi-byte input, reachable from proxy-URL validation

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/http_client.rs:327-334` — when `url::Url::parse` fails, `format!("{}...", &url[..20])` slices by byte offset; `:232-233` and `:244-245` call `mask_url(url)` precisely on the parse-failure path of `build_client`, which `validate_proxy` (`:92-97`) and `apply_proxy` (`:106-108`) run on user input from the settings UI.
- **Problem:** A pasted proxy string containing non-ASCII characters around byte 20 (for example a Chinese label or a full-width colon such as `代理服务器：http://...`) panics with 'byte index 20 is not a char boundary' inside the Tauri command instead of returning 'Invalid proxy URL'; the command never resolves and the UI shows no validation message.
- **Proposal:** Truncate by characters (`url.chars().take(20).collect::<String>()`) or use `str::floor_char_boundary`, and add a unit test with a multi-byte string.
- **Acceptance:** `mask_url("代理服务器：http://127.0.0.1:7890")` returns a truncated string without panicking; `validate_proxy` on that input returns `Err` with a readable message; the new test is part of `http_client.rs` tests.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### PROXY-47: Bedrock thinking optimizer sets budget_tokens to max_tokens - 1, overrides the caller's smaller budget or disabled thinking, and forces effort=max plus the 1M beta for every adaptive model

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/thinking_optimizer.rs:38-43` — `budget_target = max_tokens.saturating_sub(1)` (16384 when `max_tokens` is absent); `:59-69` — an explicitly configured lower `budget_tokens` is raised to that target; `:52-57` — `thinking: {type: disabled}` is overridden to enabled; `:27-32` — for adaptive models it unconditionally injects `output_config.effort = "max"` and appends `context-1m-2025-08-07`; applied per attempt whenever `optimizer_config.enabled && is_bedrock_provider` (`forwarder.rs:451-460`). No sub-switch or cap exists in `OptimizerConfig` (`types.rs` optimizer section).
- **Problem:** With a budget essentially equal to `max_tokens`, the model can spend nearly the whole output allowance thinking, so the visible answer is cut off by `max_tokens` (Claude Code sees `stop_reason: max_tokens` with little or no text); a caller that deliberately set a small budget or disabled thinking for a cheap subagent call is silently overridden; `effort: max` maximises latency and cost on every request; and the 1M beta is injected even for models/accounts without 1M access, which Bedrock rejects.
- **Proposal:** Cap the injected budget (e.g. `min(max_tokens * 3/4, 32000)`), never raise a caller-provided budget or re-enable caller-disabled thinking, and make `effort` and the 1M beta opt-in sub-switches of `OptimizerConfig` documented in the optimizer UI.
- **Acceptance:** A request with `max_tokens: 4096, thinking: {enabled, budget_tokens: 1024}` keeps budget 1024; a request without a budget gets at most 3072; `output_config`/1M beta appear only when the corresponding switches are on; tests updated accordingly.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### PROXY-48: Two write paths for auto_failover_enabled with different invariants: saving the per-app settings form can silently flip failover from a stale snapshot, bypassing the P1-switch and queue checks

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/commands/failover.rs:79-182` — `set_auto_failover_enabled` validates takeover, auto-fills the queue and switches to P1 before writing; `src-tauri/src/commands/proxy.rs:133-148` — `update_proxy_config_for_app` writes the whole `AppProxyConfig`, including `auto_failover_enabled` and `enabled`, with no such checks (`src-tauri/src/database/dao/proxy.rs:275-290`); `src/components/proxy/AutoFailoverConfigPanel.tsx:25-54`, `:162-175` — the form snapshots `config.autoFailoverEnabled` and sends it back on save; `src/lib/query/failover.ts:224-245` — the header toggle's `onSuccess` does not invalidate `["appProxyConfig", appType]`, so the form keeps the pre-toggle value. The router reads the flag on every request (`src-tauri/src/proxy/provider_router.rs:43-49`), so the routing mode changes immediately.
- **Problem:** Toggle failover on in the header, then save a timeout change in the Auto-Failover panel: failover is written back to `false` (or, in the other direction, to `true` without the P1 switch and queue auto-fill) with a 'saved' toast and no indication; the proxy's routing mode changes underneath the user. The same path can flip `enabled` (takeover) without the live-config rewrite the takeover command performs.
- **Proposal:** Make `auto_failover_enabled` and `enabled` non-writable through `update_proxy_config_for_app` (ignore or reject them; the DAO already has a dedicated statement for those two flags at `dao/proxy.rs:892`), drop the field from the form payload, and invalidate `["appProxyConfig", appType]` in `useSetAutoFailoverEnabled.onSuccess`.
- **Acceptance:** Saving the Auto-Failover panel after toggling failover elsewhere leaves the toggle state unchanged; a backend test asserts `update_proxy_config_for_app` cannot change `auto_failover_enabled` or `enabled`.
- **Verification:** added by completeness-critic pass (not independently re-verified)
