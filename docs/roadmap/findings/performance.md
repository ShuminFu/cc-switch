# Performance & resources — findings (PERF-xx)

Part of the [next-phase proposals](../README.md). 40 findings. Priorities: P0 = 2, P1 = 15, P2 = 15, P3 = 8.

## Area summary

The codebase (147.7k lines of Rust, 81.3k lines of TS/TSX, 750 crates, v3.17.0) has no performance instrumentation at all: no `benches/`, no criterion, no load-test harness, no startup timing spans, no bundle report, and CI (`.github/workflows/ci.yml`) only runs typecheck/format/unit tests. The biggest hot spots are in the proxy request path and the database layer: every Claude-format upstream request opens a brand-new TCP+TLS connection (`src-tauri/src/proxy/hyper_client.rs:373-432`), every request deep-clones the full JSON body 3-5 times and rebuilds the tree twice (`forwarder.rs:451-463, 1160-1169, 1527-1535`), and every request performs 6-8 SQLite statements under one global `std::sync::Mutex<Connection>` with no WAL, no `synchronous=NORMAL`, no statement cache and no transaction batching (`database/mod.rs:79-81, 109-116`; `proxy/usage/logger.rs:51-124, 220-312`). That same mutex is also held for long windows by the 60-second session-log sync, the startup rollup/backfill, and UI dashboard aggregates that run on the main thread as synchronous Tauri commands (`commands/usage.rs:12-116`), so the proxy's logger, the tray rebuild and the UI all serialize on it. Quick wins (hours each): enable WAL + `synchronous=NORMAL` + `busy_timeout`, add a `prepare_cached` pass, fold the SSE usage collector into an accumulator instead of retaining every chunk as a `serde_json::Value`, make the heavy commands `async`, lazy-load the four locale files (660 KB) and the 11k-line preset modules, and debounce the usage dashboard invalidation. Larger refactors worth planning: a dedicated usage-ingestion writer task with batching plus a WAL reader pool, a pooled keep-alive transport for the header-case-preserving path, an mtime-indexed session catalogue in SQLite, and route-level code splitting. Each finding below includes a measurement so the next phase can land with numbers; PERF-30 consolidates the measurement plan (criterion transforms/SSE benches, oha/k6 proxy load harness, startup spans, rollup-plugin-visualizer, `EXPLAIN QUERY PLAN` checks).

## Index

| ID | Title | Type | Priority | Effort | Confidence |
|---|---|---|---|---|---|
| [PERF-01](#perf-01) | Header-case-preserving upstream path performs a full TCP + TLS handshake on every request (no connection reuse) | performance | P0 | L | high |
| [PERF-02](#perf-02) | SQLite opened with default journal/sync settings behind one global std Mutex (no WAL, synchronous=FULL, no busy_timeout, no cache tuning) | performance | P0 | S | high |
| [PERF-03](#perf-03) | Usage logging executes 5-6 synchronous SQL statements per request on Tokio worker threads, with no batching, no statement cache and no writer task | refactor | P1 | M | high |
| [PERF-04](#perf-04) | Every request deep-clones the full JSON body 3-5 times and rebuilds the tree twice before serialization | performance | P1 | M | high |
| [PERF-05](#perf-05) | 6-8 database/config lookups per request before the first upstream byte (per-app proxy config read twice, full providers table scanned and JSON-parsed) | performance | P1 | M | high |
| [PERF-06](#perf-06) | SSE passthrough parser allocates per line and memmoves the buffer per event | performance | P1 | S | high |
| [PERF-07](#perf-07) | Streaming usage collector retains every Gemini/OpenAI chunk as a parsed `serde_json::Value` for the whole stream | performance | P1 | S | high |
| [PERF-08](#perf-08) | Cross-source dedup `NOT EXISTS` correlated subquery is re-evaluated inside every dashboard aggregate | performance | P1 | M | medium |
| [PERF-09](#perf-09) | Provider limit checks wrap the indexed `created_at` column in `date()`/`strftime()` (non-sargable full scans) | performance | P1 | S | high |
| [PERF-10](#perf-10) | Session-usage sync (every 60 s) issues one SELECT per JSONL file, one lock+INSERT per message with no transaction, and a fuzzy pricing lookup per message | performance | P1 | M | high |
| [PERF-11](#perf-11) | Startup does all configuration imports, DB maintenance and the full tray build synchronously inside `setup` before the window is shown, with no timing | performance | P1 | M | high |
| [PERF-12](#perf-12) | 100 of 291 Tauri commands are synchronous (`pub fn`), including all dashboard aggregates, provider listing and skill scanning, so they execute on the main thread | performance | P1 | S | medium |
| [PERF-13](#perf-13) | Idle polling storm: five overlapping 2-5 s React Query pollers plus a 10 s providers refetch, each hitting the global DB mutex and reading proxy config four times | performance | P1 | M | high |
| [PERF-14](#perf-14) | Usage dashboard refetches all six queries on every `usage-log-recorded` event (200 ms backend debounce only) in addition to 30 s polling | performance | P1 | S | high |
| [PERF-15](#perf-15) | No code splitting: one monolithic bundle with every page, CodeMirror, recharts, flexsearch, 11k lines of presets and 135 KB of inline SVG strings | performance | P1 | M | high |
| [PERF-16](#perf-16) | Tray menu is rebuilt from scratch on every provider/profile/failover change, re-reading every provider table, blocking on async DB calls and parsing live config files from disk | performance | P1 | M | high |
| [PERF-17](#perf-17) | Single-connection design means long maintenance windows (rollup, backfill, session sync, backup) block the proxy logger and all UI commands | refactor | P1 | L | high |
| [PERF-18](#perf-18) | Streaming converters build a `serde_json::Value` per event, serialize to `String`, then `format!` the SSE frame into a second `String` (2-3 copies per token delta) | performance | P2 | M | high |
| [PERF-19](#perf-19) | All four locale bundles (660 KB raw JSON) are statically imported and shipped in the main chunk | performance | P2 | S | high |
| [PERF-20](#perf-20) | Provider cards are not memoized and receive a new inline callback every render while the parent re-renders every 2 s | performance | P2 | S | medium |
| [PERF-21](#perf-21) | Recharts trend chart re-renders with full data on every 30 s refetch/event and defines its tooltip component inline | performance | P2 | S | medium |
| [PERF-22](#perf-22) | Session manager rescans every session root on each open and parses whole transcripts into `serde_json::Value`; search index rebuilt with the most expensive tokenizer | performance | P2 | L | high |
| [PERF-23](#perf-23) | Request-log listing uses OFFSET pagination with a COUNT(*) join per page and performs cost backfill writes inside the read path | performance | P2 | M | high |
| [PERF-24](#perf-24) | Startup runs rollup+prune, full-table cost backfill, pricing seed, stream-check cleanup and `incremental_vacuum` synchronously inside `Database::init` | performance | P2 | S | high |
| [PERF-25](#perf-25) | Cargo.lock contains two complete HTTP stacks and many duplicated crates (750 packages) | tech-debt | P2 | M | high |
| [PERF-26](#perf-26) | Feature and profile choices trade proxy throughput for size: `hyper = full`, two root-certificate stores built separately, `opt-level = "s"` for the whole crate | tech-debt | P2 | S | medium |
| [PERF-27](#perf-27) | `serde_json` `preserve_order` turns every parsed object into an `IndexMap`, inflating memory and clone cost on the proxy hot path | performance | P2 | M | medium |
| [PERF-28](#perf-28) | Debug-level logging formats and writes every request body and every SSE event to a 1 GB log file | performance | P2 | S | high |
| [PERF-29](#perf-29) | Frontend dependency weight: framer-motion for simple fades, 14 Radix packages, recharts 3, CodeMirror 6 with four language packs, flexsearch, cmdk; no bundle analysis in CI | tech-debt | P2 | M | medium |
| [PERF-30](#perf-30) | No performance measurement infrastructure (benchmarks, load harness, startup spans, bundle/DB plan checks) | test | P2 | M | high |
| [PERF-31](#perf-31) | No SQLite statement cache: 310 `conn.execute`/`prepare` call sites, zero `prepare_cached` | performance | P2 | S | high |
| [PERF-33](#perf-33) | Frontend boot waterfall: render blocked on `get_init_error`, then ~10 independent invokes including a duplicated env-conflict scan | performance | P2 | S | high |
| [PERF-32](#perf-32) | Global `invalidateQueries()` with no key refetches every mounted query (8 sites) and 164 invalidation sites in total | performance | P3 | S | high |
| [PERF-34](#perf-34) | Each accepted connection allocates an 8 KB peek buffer and re-parses the request head; request bodies are fully buffered with a 200 MB ceiling | performance | P3 | S | high |
| [PERF-35](#perf-35) | `notify_log_recorded` spawns an OS thread per debounce window and the DB `update_hook` runs for every row change | performance | P3 | S | high |
| [PERF-36](#perf-36) | Six indexes (incl. three single-column ones) and a TEXT UUID primary key on the append-only `proxy_request_logs` table add write amplification | performance | P3 | M | medium |
| [PERF-37](#perf-37) | Non-streaming responses are always fully buffered, cloned and decompressed on the proxy even when no parsing or transform is needed | performance | P3 | S | medium |
| [PERF-38](#perf-38) | `CodexChatHistoryStore` caps entries (512 responses) but not bytes, and `GeminiShadowStore` defaults are not surfaced for tuning | performance | P3 | S | low |
| [PERF-39](#perf-39) | `scan_unmanaged_skills` walks 8+ directories and reads every `SKILL.md` synchronously on the main thread when the Skills page opens | performance | P3 | S | medium |
| [PERF-40](#perf-40) | `ProviderRouter::select_providers` allocates a circuit-breaker key string and takes an async RwLock per provider per request even when failover is disabled for all but one provider | performance | P3 | S | medium |

## Findings

### PERF-01: Header-case-preserving upstream path performs a full TCP + TLS handshake on every request (no connection reuse)

**Type:** performance · **Priority:** P0 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/hyper_client.rs:373-432` — `send_raw_request` always does `tokio::net::TcpStream::connect((host, port))` (line 400) and a fresh rustls handshake `tls_connector.connect(server_name, stream)` (lines 410-411), then `do_hyper_response` (666-700) runs a brand-new `hyper::client::conn::http1` handshake and spawns a connection driver task per request (682). `src-tauri/src/proxy/forwarder.rs:2117-2176` selects this path for every request where `should_preserve_exact_header_case` is true, which per `forwarder.rs:3141-3157` is every Claude-app request to an Anthropic-format provider (the most common deployment: Claude Code -> Anthropic-compatible relay); the comment at 2117-2120 explicitly acknowledges the raw path "re-handshakes on every request". The pooled fallback `global_hyper_client()` (59-70) is only used when no original header cases were captured, and it is HTTP/1.1 only (`enable_http1()`, line 65). The reqwest client is pooled (`http_client.rs:216-227`, `pool_max_idle_per_host(10)`) but is not used on this path.
- **Problem:** Each Claude Code turn pays one TCP RTT plus one or two TLS RTTs (typically 80-300 ms to a remote relay, more over a corporate HTTP proxy where `connect_via_proxy` adds a CONNECT round trip, lines 440-562) before the first byte is sent, and the OS accumulates TIME_WAIT sockets. For agentic sessions that issue dozens of requests per minute this is the single largest avoidable contributor to time-to-first-token.
- **Proposal:** Add a small keyed pool (host, port, proxy_url) of idle `TlsStream`s with an idle timeout (~60 s) and reuse HTTP/1.1 keep-alive connections once the previous response body has been fully read; alternatively verify that hyper-util's `http1_preserve_header_case(true)` + the `HeaderCaseMap` extension already inserted at `server.rs:178-192` reproduces the wire casing (order is the only remaining difference) and route the common case through the pooled client, keeping raw writes only for providers that demonstrably need exact ordering. Expected: TTFB drop of 1-2 RTT per request (100-300 ms), far fewer ephemeral sockets.
- **Acceptance:** With a local TLS echo upstream, `oha -n 500 -c 8` against `/v1/messages` shows p50 TTFB improved by at least one RTT; `ss -tan | grep TIME-WAIT` count stays flat during the run; rustls debug logs show one handshake per pooled connection instead of per request.

### PERF-02: SQLite opened with default journal/sync settings behind one global std Mutex (no WAL, synchronous=FULL, no busy_timeout, no cache tuning)

**Type:** performance · **Priority:** P0 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/database/mod.rs:79-81` — `pub struct Database { conn: Mutex<Connection> }` (std mutex, single connection); `mod.rs:109-116` only executes `PRAGMA foreign_keys = ON` and `PRAGMA auto_vacuum = INCREMENTAL`; `rg journal_mode|synchronous|cache_size|mmap|busy_timeout` finds nothing for the main DB (only codex/opencode external DBs set a busy_timeout). `mod.rs:64-70` `lock_conn!` is used at 197 sites; every DAO call locks the whole connection.
- **Problem:** With `journal_mode=DELETE` + `synchronous=FULL`, every `INSERT INTO proxy_request_logs` (one per proxied request plus one per imported session message) costs two fsyncs and takes a file lock that excludes readers, and because the Rust-level mutex is held across the entire statement, every UI query (dashboard aggregates, provider list refresh every 10 s, tray rebuild) blocks the proxy's logger and vice versa. On slow/encrypted disks or when the DB lives on a network share this manifests as hundreds of milliseconds of stall per request.
- **Proposal:** On open, run `PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000; PRAGMA cache_size=-16000; PRAGMA temp_store=MEMORY; PRAGMA mmap_size=268435456;` (WAL is persistent, the rest per connection). Follow up with PERF-17 (reader pool) so readers never queue behind the writer.
- **Acceptance:** A micro-benchmark inserting 10k `proxy_request_logs` rows drops from seconds to well under 500 ms; `PRAGMA journal_mode` returns `wal` on an upgraded DB; dashboard query p95 under concurrent inserts (k6 at 20 req/s) stays below 50 ms.

### PERF-03: Usage logging executes 5-6 synchronous SQL statements per request on Tokio worker threads, with no batching, no statement cache and no writer task

**Type:** refactor · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/usage/logger.rs:220-312` `resolve_pricing_config` issues `get_default_cost_multiplier` (dao/proxy.rs:124-142), `get_pricing_model_source` (dao/proxy.rs:170-188) and `get_provider_by_id` (dao/providers.rs:130-160, which JSON-parses `settings_config` and `meta` on every call); `logger.rs:316-333` then calls `get_model_pricing` (206-217, fuzzy lookup via `find_model_pricing_row` which loops candidate names with exact + `LIKE` queries at `services/usage_stats.rs:1958-1975, 2007-2050`); `logger.rs:51-124` finally does `INSERT OR REPLACE` with `conn.execute` (no `prepare_cached`; the repo has 0 `prepare_cached` sites vs 310 `conn.execute`/`prepare` sites). The work runs inside `tokio::spawn` (`response_processor.rs:507, 533, 586`) but blocks the worker thread on the std mutex; `handlers.rs:2351-2383` `log_forward_error` runs the insert inline in the request path (called at `handlers.rs:204-212`), and `handlers.rs:2385-2432` `log_usage` is awaited inline for the Claude transform path.
- **Problem:** Each request compiles ~6 SQL statements and takes the global lock ~6 times; when the dashboard or session sync holds the lock, Tokio worker threads stall and streaming responses for other clients hiccup (the executor cannot poll their streams).
- **Proposal:** Introduce a `UsageIngest` actor: an `mpsc` channel of `RequestLog`, a single task that drains up to N entries and writes them in one transaction with `prepare_cached`, runs on `spawn_blocking`; cache `(provider_id, app_type) -> (multiplier, pricing_source)` and `model -> pricing` in a `moka`/`HashMap` with invalidation on settings/pricing mutations (both have single write paths). Expected: proxy path no longer touches SQLite; insert cost amortized across batches.
- **Acceptance:** Tokio runtime metrics (`tokio-console` or `RuntimeMetrics::worker_busy`) show no blocking spikes under 20 req/s; log write latency measured from `finish()` to row visibility stays under 100 ms; zero `lock_conn!` calls reachable from `response_processor.rs`.

### PERF-04: Every request deep-clones the full JSON body 3-5 times and rebuilds the tree twice before serialization

**Type:** performance · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/handlers.rs:200` passes `body.clone()` into `forward_with_retry`; `forwarder.rs:451-463` clones `body` again per provider attempt (`body.clone()` at 462 even on the non-Bedrock path); `forwarder.rs:1160-1164` `map_proxy_request_model(body.clone(), provider)` / `apply_model_mapping(body.clone(), provider)`; `forwarder.rs:1169` `normalize_thinking_type(mapped_body)` consumes and rebuilds; `forwarder.rs:1527-1535` `prepare_upstream_request_body` = `filter_private_params_with_whitelist` (body_filter.rs:68-118, rebuilds every object/array) followed by `canonicalize_value` (full-tree key sort/rebuild), executed twice when the media rectifier retries; `forwarder.rs:2053` `serde_json::to_vec`; `hyper_client.rs:589-656` copies the serialized body into a new `Vec` (`raw.extend_from_slice(body)`). `Cargo.toml:26` enables `serde_json` `preserve_order`, so each object is an `IndexMap` (heavier to clone).
- **Problem:** Claude Code request bodies are routinely 200 KB-2 MB (full conversation, tool schemas, cached system prompt). Five deep clones plus two rebuilds mean ~10 MB of allocation churn and several milliseconds of CPU per request, on the Tokio worker threads that also drive streaming for other clients; it also multiplies peak RSS under concurrency.
- **Proposal:** Keep the parsed body in an `Arc<Value>` and clone only when a mutation actually applies (`model_mapper`, rectifiers, Bedrock optimizer already return `bool`/results that can gate the clone); make `filter_private_params` and `canonicalize_value` in-place (`IndexMap::sort_keys`, `Map::retain`); for passthrough providers (no transform, no mapping) forward the original `Bytes` unchanged. Expected: 2-4x less allocation per request; measurable in a criterion bench.
- **Acceptance:** criterion bench `prepare_request(body_500kb)` shows allocations (via `dhat` or `counting-allocator`) reduced by at least 60 %; peak RSS of the proxy under `oha -c 32` with 500 KB bodies drops accordingly.

### PERF-05: 6-8 database/config lookups per request before the first upstream byte (per-app proxy config read twice, full providers table scanned and JSON-parsed)

**Type:** performance · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/handler_context.rs:98-108` reads `get_proxy_config_for_app` (dao/proxy.rs:215-250), `get_rectifier_config`, `get_optimizer_config`, `get_copilot_optimizer_config` on every request; `handler_context.rs:134-144` calls `select_providers`, which at `provider_router.rs:43` reads `get_proxy_config_for_app` again, then either `get_all_providers` + `get_failover_queue` (53-61; `get_all_providers` deserializes `settings_config` and `meta` JSON for every provider row, dao/providers.rs:20-60) or `get_effective_current_provider` (settings.rs:1003-1030, which itself calls `get_all_providers`) + `get_provider_by_id` (provider_router.rs:81-95).
- **Problem:** Each request takes the global mutex 6-8 times and re-parses every provider's JSON blobs; these are all configuration values that change only through a handful of UI mutations.
- **Proposal:** Hold `AppProxyConfig`, rectifier/optimizer configs and the per-app provider snapshot in `ProxyState` behind `Arc<ArcSwap>`/`RwLock` with a generation counter bumped by the existing mutation paths (`apply_runtime_config` at `server.rs:373-375` already exists for the global config). Expected: zero DB reads on the request path.
- **Acceptance:** Instrument `RequestContext::new` with an `Instant` span logged at debug; p50 drops from milliseconds (lock contention) to microseconds; `lock_conn!` count reachable from `handler_context.rs`/`provider_router.rs` is zero.

### PERF-06: SSE passthrough parser allocates per line and memmoves the buffer per event

**Type:** performance · **Priority:** P1 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/sse.rs:2-5` `strip_sse_field` builds two `format!("{field}: ")`/`format!("{field}:")` strings on every call; `sse.rs:8-23` `take_sse_block` runs two `find`s over the whole buffer and `buffer.drain(..pos+len)` (O(n) memmove) per event; `response_processor.rs:690-691` enables inspection whenever usage logging is on (default) or debug logging is enabled, and `response_processor.rs:745-775` calls `strip_sse_field` for every line of every event (plus `event_text.trim()`/`lines()` allocations). The same helpers are used by every streaming converter (`providers/streaming.rs:181-186`, `streaming_responses.rs`, `streaming_codex_chat.rs`, `streaming_gemini.rs`).
- **Problem:** A long answer (~1000 SSE events, 50-200 KB) generates ~4000 short-lived `String`s and ~1000 buffer memmoves per response, on the same worker threads that serve other streams; CPU per token is dominated by this bookkeeping rather than by forwarding.
- **Proposal:** Use `&'static` prefixes (`line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:"))`), scan with a read cursor and only `drain` when the cursor passes half the buffer, use `memchr::memmem` for the delimiter search, and keep the buffer as `BytesMut` (append_utf8_safe already handles split code points). Expected: 3-5x less CPU in the passthrough loop.
- **Acceptance:** criterion bench `passthrough(recorded_claude_stream_200kb)` shows >=3x throughput; `perf`/`samply` profile of the proxy under `oha` shows `take_sse_block`/`strip_sse_field` below 2 % of samples.

### PERF-07: Streaming usage collector retains every Gemini/OpenAI chunk as a parsed `serde_json::Value` for the whole stream

**Type:** performance · **Priority:** P1 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/handler_config.rs:46-56` pre-filters on `data.contains("\"usage\"")` (OpenAI) and `data.contains("\"usageMetadata\"")` (Gemini); Gemini emits `usageMetadata` in every chunk, and OpenAI emits `"usage":null` in every chunk once `stream_options.include_usage` is set, which `providers/transform.rs:241-271` injects. `response_processor.rs:347-355` stores `events: Mutex<Vec<Value>>`, `398-402` pushes each parsed event, `754-756` parses with `serde_json::from_str::<Value>`; the parsers only need the last usage object and first model id (`usage/parser.rs:412-425` iterates in reverse for the last non-null usage; `458-500` overwrites totals per chunk).
- **Problem:** Memory per in-flight stream equals the entire response materialized as a `Value` tree (3-5x the text size, with `preserve_order` IndexMaps), freed only at stream end; with several concurrent long Gemini/OpenAI-compatible streams this adds tens of MB and GC-like latency spikes at `finish()`.
- **Proposal:** Replace the `Vec<Value>` with a `UsageAccumulator { last_usage: Option<TokenUsage>, model: Option<String>, message_id: Option<String> }` updated per event (the filter + parse of each event can stay, the retention goes away); drop the two `tokio::sync::Mutex` locks per event (355, 398-402) for atomics/`parking_lot`.
- **Acceptance:** Heap profile (`dhat`) during a 500 KB Gemini stream shows collector memory bounded by a few KB; unit tests for the four parsers pass against accumulated state.

### PERF-08: Cross-source dedup `NOT EXISTS` correlated subquery is re-evaluated inside every dashboard aggregate

**Type:** performance · **Priority:** P1 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/usage_stats.rs:296-330` builds `effective_usage_log_filter` (a correlated `EXISTS` over `proxy_request_logs` with `LOWER(model)` comparisons and an `OR` on `cache_creation_tokens`) that is appended to the summary (596-612), per-app (758-772), trends (1007-1019), provider stats (1263-1281), model stats (1415-1430) and list (1512-1548) queries, and to `rollup_and_prune` (dao/usage_rollup.rs:118). The expression index at `schema.rs:2812-2820` covers `(app_type, COALESCE(data_source,'proxy'), input_tokens, output_tokens, cache_read_tokens, created_at, cache_creation_tokens)` but the `LOWER(model)` disjunction and the `OR` branch are not index-friendly. The equivalent point lookup `has_matching_proxy_usage_log` (372-420) already exists.
- **Problem:** With session-log rows present (every Claude Code user after the first sync), each dashboard query becomes O(session_rows x candidate_proxy_rows) and all six queries run every 30 s and on every usage event (PERF-14), on the main thread (PERF-12), holding the global mutex.
- **Proposal:** Decide dedup once at insert time: when the session sync imports a message, call `has_matching_proxy_usage_log` and store a `dedup_hidden INTEGER` (or `data_source='session_log_dup'`) column; the proxy logger can symmetrically mark the matching session row. All read queries then filter `WHERE dedup_hidden = 0`. Keep a one-off backfill migration.
- **Acceptance:** `EXPLAIN QUERY PLAN` for the summary query shows no `CORRELATED SCALAR SUBQUERY` and uses `idx_request_logs_app_created_at`; on a synthetic 200k-row DB (50 % session rows) summary + trends + stats run in < 50 ms total vs. seconds today.

### PERF-09: Provider limit checks wrap the indexed `created_at` column in `date()`/`strftime()` (non-sargable full scans)

**Type:** performance · **Priority:** P1 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/usage_stats.rs:1631-1665` — `WHERE provider_id = ? AND app_type = ? AND date(datetime(created_at,'unixepoch','localtime')) = date('now','localtime')` and `strftime('%Y-%m', datetime(created_at, ...)) = strftime('%Y-%m','now','localtime')`, so `idx_request_logs_created_at`/`idx_request_logs_app_created_at` (schema.rs:215, 2783) cannot bound the range; only `idx_request_logs_provider` narrows it. The same `date(l.created_at, 'unixepoch', 'localtime')` bucketing appears in trends (1007) and rollup (dao/usage_rollup.rs:148) but those at least have `created_at` range predicates.
- **Problem:** `check_provider_limits` is called from the proxy path when limits are configured and from the UI; each call scans every row of the provider and evaluates two datetime conversions per row.
- **Proposal:** Compute local-midnight and month-start epoch bounds in Rust (the helper `compute_local_midnight_cutoff` already exists at `dao/usage_rollup.rs:63`) and filter `created_at >= ?`; add an index on `(provider_id, app_type, created_at)` or reuse the existing composite order.
- **Acceptance:** `EXPLAIN QUERY PLAN` shows `SEARCH ... USING INDEX ... (provider_id=? AND app_type=? AND created_at>?)`; query time on 200k rows < 2 ms.

### PERF-10: Session-usage sync (every 60 s) issues one SELECT per JSONL file, one lock+INSERT per message with no transaction, and a fuzzy pricing lookup per message

**Type:** performance · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/lib.rs:1093-1140` runs `sync_claude_session_logs` + codex + gemini + opencode syncs at startup and every 60 s; `services/session_usage.rs:61-120` walks the whole `~/.claude/projects` tree each pass (`collect_jsonl_files`, 121-180), `182-200` does `fs::metadata` + `get_sync_state` (a SELECT, 362-372) per file; `410-520` `insert_session_log_entry` takes the mutex and runs an `INSERT OR IGNORE` per message after `find_model_pricing_for_session` (525-530 -> `usage_stats.rs:1958-1975` candidate loop with exact + `LIKE` queries); `388-408` `update_sync_state` is another statement per changed file. No transaction wraps a file. The DB `update_hook` (`database/mod.rs:83-92`) and `notify_log_recorded` (session_usage.rs:514-516) fire per inserted row.
- **Problem:** Users with thousands of transcripts (common after months of Claude Code use) pay thousands of statements per minute forever, each taking the global mutex; a first sync of a large history holds the writer busy for minutes while the proxy logger and UI queue behind it; the per-insert fsync (PERF-02) makes it worse.
- **Proposal:** Load `session_log_sync` into a `HashMap<path,(mtime,offset)>` once per pass; skip project directories whose own mtime is unchanged; wrap each file's inserts in one transaction; memoize `model -> pricing` per pass; move the walk to `spawn_blocking`; longer term use a filesystem watcher (`notify`) for the Claude/Codex roots so idle passes cost nothing.
- **Acceptance:** With 2,000 unchanged JSONL files, a periodic pass issues <= 2 SQL statements and completes in < 50 ms (log the pass duration at debug); first-time import of 50k messages completes in < 5 s.

### PERF-11: Startup does all configuration imports, DB maintenance and the full tray build synchronously inside `setup` before the window is shown, with no timing

**Type:** performance · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/tauri.conf.json:22` creates the window `visible: false`; `src-tauri/src/lib.rs:318-1190` `setup` runs sequentially: log init (338-368), `Database::init` (453; see PERF-25), JSON migration, `import_default_config` for every app type (561-600, each reads live config files), `init_default_official_providers` (602-608), OpenCode/OpenClaw/Hermes live imports (686-705), two `get_all_providers("opencode")` scans for OMO (708-744), MCP and prompt imports (747-837), deep-link registration, then `create_tray_menu` (911; PERF-16, which reads every provider table and live config files again) and only then `window.show()` (1183). No phase is timed.
- **Problem:** Cold start is gated by disk I/O and SQLite maintenance that the user does not need before seeing the provider list; on a large DB or slow disk the window appears seconds late with no feedback.
- **Proposal:** Show the window (or a shell) immediately after `manage(app_state)`; move imports, OMO/MCP/prompt seeding, the session sync kick-off and the tray build into the existing post-setup `tauri::async_runtime::spawn` block (1042+) and use `spawn_blocking` for file/DB work; add `Instant`-based spans per phase logged at info (`startup.phase=db_init ms=...`) and surface the total in the About page.
- **Acceptance:** Time from process start to `window.show()` (log timestamp delta) < 300 ms on a warm cache with a 100 MB DB; phase breakdown visible in `cc-switch.log`.

### PERF-12: 100 of 291 Tauri commands are synchronous (`pub fn`), including all dashboard aggregates, provider listing and skill scanning, so they execute on the main thread

**Type:** performance · **Priority:** P1 · **Effort:** S · **Confidence:** medium

- **Evidence:** `rg "^pub fn " src-tauri/src/commands/*.rs` = 100 sync commands vs 191 `pub async fn`; 0 uses of `#[tauri::command(async)]`. Heavy examples: `commands/usage.rs:12-116` (`get_usage_summary`, `get_usage_summary_by_app`, `get_usage_trends`, `get_provider_stats`, `get_model_stats`, `get_request_logs` — all run the dedup-heavy queries of PERF-08 under the global mutex), `commands/provider.rs:23-32` (`get_providers`, polled every 10 s when the proxy runs, `src/lib/query/queries.ts:62-67`), `commands/skill.rs:102-106` (`scan_unmanaged_skills` walks 8+ directories, `services/skill.rs:1377-1410`). Tauri 2 documents that non-`async` commands without `#[tauri::command(async)]` run on the main thread.
- **Problem:** Any lock wait or slow query inside these commands freezes the webview event loop and tray; combined with PERF-14 the dashboard can block the UI several times per second while the proxy is active.
- **Proposal:** Mark DB-backed commands `async` and run the DAO call inside `tauri::async_runtime::spawn_blocking` (22 sites already do this for sessions), or annotate with `#[tauri::command(async)]`; keep trivial getters sync.
- **Acceptance:** No `pub fn` command in `commands/usage.rs`, `provider.rs`, `skill.rs` touches `lock_conn!` synchronously; UI frame timing (Chrome DevTools performance trace in the webview) shows no main-thread tasks > 50 ms during a 20 req/s proxy load.

### PERF-13: Idle polling storm: five overlapping 2-5 s React Query pollers plus a 10 s providers refetch, each hitting the global DB mutex and reading proxy config four times

**Type:** performance · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/hooks/useProxyStatus.ts:24-38` polls `get_proxy_status` every 2 s and `get_proxy_takeover_status`; `src/lib/query/proxy.ts:12-50` polls `proxyStatus` every 5 s (same key, different interval), `proxyRunning` 2 s, `liveTakeoverActive` 2 s, `proxyTakeoverStatus` 2 s; `src/lib/query/failover.ts:17,88` poll every 5 s; `src/lib/query/queries.ts:62-67` refetches providers (two invokes) every 10 s while the proxy runs. Backend: `services/proxy.rs:686-716` `get_takeover_status` performs four `get_proxy_config_for_app` reads, and `2139-2142` `is_takeover_active` calls it again. `src/lib/query/queryClient.ts:5-9` sets `staleTime: 0` and `refetchOnWindowFocus: true` globally, so every window focus refetches all mounted queries.
- **Problem:** ~4-6 IPC round trips and ~10 mutex acquisitions per second while idle, competing with the proxy logger (PERF-02/03); CPU wakeups keep laptops from idling; focus changes trigger bursts of dozens of invokes.
- **Proposal:** Replace polling with push: emit `proxy-status-changed`/`takeover-changed` events from `ProxyService` state transitions (an event bridge already exists for usage), expose one `get_proxy_overview` command returning status+running+takeover in a single call, dedupe the two `proxyStatus` hooks, and set a default `staleTime` of 5-10 s with `refetchOnWindowFocus: false` except for explicitly fresh data.
- **Acceptance:** With the main window open and proxy idle, `log::debug` invoke counter shows < 0.5 commands/s; CPU of the webview process at idle < 1 %.

### PERF-14: Usage dashboard refetches all six queries on every `usage-log-recorded` event (200 ms backend debounce only) in addition to 30 s polling

**Type:** performance · **Priority:** P1 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/hooks/useUsageEventBridge.ts:23-27` invalidates `usageKeys.all` on every event; the backend debounce is 200 ms (`src-tauri/src/usage_events.rs:23`) and the event fires after each proxy log write (`proxy/usage/logger.rs:121`) and each imported session row (`services/session_usage.rs:514-516`); `src/lib/query/usage.ts:186,213,242,271,300,329` additionally poll summary/by-app/trends/provider stats/model stats/logs every 30 s; `src/components/usage/UsageDashboard.tsx:120-126` also invalidates on filter change.
- **Problem:** During an active agent session (~1 request/s) the dashboard reruns 6 full-table aggregate queries (PERF-08) up to 5x per second, on the main thread (PERF-12), holding the DB mutex that the proxy logger needs.
- **Proposal:** Debounce/throttle invalidation in the bridge (>= 2 s, trailing), only invalidate queries for the visible tab, and have the backend include a monotonically increasing `log_version` so queries can short-circuit when nothing changed; keep polling as a fallback at >= 60 s.
- **Acceptance:** Under a 1 req/s proxy load with the dashboard open, `get_usage_*` commands execute <= 6 times per 2 s window; SQLite profile (`sqlite3_profile`/`log::trace` per query) shows aggregate query count reduced by > 80 %.

### PERF-15: No code splitting: one monolithic bundle with every page, CodeMirror, recharts, flexsearch, 11k lines of presets and 135 KB of inline SVG strings

**Type:** performance · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `vite.config.ts:1-31` has no `build.rollupOptions.output.manualChunks` and no analyzer; `rg "React.lazy|lazy\(" src` finds zero route/component-level lazy loads (only three `await import()` for the updater plugin and `subscriptionApi`); `src/App.tsx:1-95` statically imports `SettingsPage` (which imports `UsageDashboard` -> recharts, `SettingsPage.tsx:51`), `SessionManagerPage` (-> flexsearch), `SkillsPage`, `UnifiedMcpPanel`, `PromptPanel`, `AgentsPanel`, `UniversalProviderPanel`, workspace/openclaw/hermes panels, `AddProviderDialog`/`EditProviderDialog` -> `ProviderForm` -> CodeMirror 6 (`JsonEditor.tsx:1-8`, 13 import sites) and `IconPicker` -> `src/icons/extracted/index.ts` (135,714 bytes of inline `<svg>` template strings plus 26 raster imports; `IconPicker.tsx:6-7`, `BasicFormFields.tsx:22`). Provider preset modules total 11,099 lines (`src/config/openclawProviderPresets.ts` 2509, `opencodeProviderPresets.ts` 2054, `codexProviderPresets.ts` 1522, `hermesProviderPresets.ts` 1527, `claudeProviderPresets.ts` 1396, `claudeDesktopProviderPresets.ts` 1145, `geminiProviderPresets.ts` 461) imported eagerly by the form fields (`ProviderForm.tsx:30-34`, `ClaudeFormFields.tsx:66`, `HermesFormFields.tsx:52`, ...).
- **Problem:** The webview must download/parse/compile the whole application (likely well above 2 MB of JS before gzip) before the first provider card renders, although the user usually only needs the provider list; on Linux WebKitGTK with hardware acceleration disabled (`lib.rs:1156-1166`) parse/compile time is significant.
- **Proposal:** `React.lazy` + `Suspense` for Settings, Usage, Sessions, Skills, MCP, Prompts, Agents, Universal, Workspace and the provider dialogs; `manualChunks` for `react`, `@radix-ui/*`, `recharts`, `@codemirror/*`, `framer-motion`, `i18next`; lazy-import the per-app preset module when its form mounts; add `rollup-plugin-visualizer` and a size budget check to CI.
- **Acceptance:** `vite build` reports an entry chunk <= 500 KB (pre-gzip) with route chunks loaded on demand; webview `performance.timing` from navigation start to first provider card rendered improves by >= 30 % on the Linux CI runner.

### PERF-16: Tray menu is rebuilt from scratch on every provider/profile/failover change, re-reading every provider table, blocking on async DB calls and parsing live config files from disk

**Type:** performance · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/tray.rs:908-920` `refresh_tray_menu` always calls `create_tray_menu` (637-875): `futures::executor::block_on(app_state.proxy_service.is_running())` (675), and per visible app section `get_all_providers` (684), `get_effective_current_provider` (687 -> another `get_all_providers`, settings.rs:1003-1016), `futures::executor::block_on(app_state.db.get_live_backup(...))` (712) and `detect_takeover_in_live_config_for_app` (716-718 -> reads and JSON-parses the live settings file, `services/proxy.rs:1857-1877, 2776-2790`), plus `get_all_profiles` (771). `update_tray_usage_labels` (875-906) reads all providers per section again. Call sites: `commands/profile.rs:92`, `commands/provider.rs:414` (via `schedule_tray_refresh`), `proxy/failover_switch.rs:113`, `commands/failover.rs:175`, `lib.rs:193-199` (`update_tray_menu` command), `lightweight.rs:28,53,93`. `block_on` is executed from Tauri command/runtime threads, which can deadlock or stall the async runtime.
- **Problem:** Each rebuild performs ~16 provider-table scans with JSON parsing, 4 file reads and hundreds of menu-item allocations; on macOS it also closes any open tray menu (acknowledged at 1005). With the 10 s providers refetch and failover auto-switch events this can run several times a minute.
- **Proposal:** Keep an in-memory `TrayModel` (per app: provider list snapshot, current id, takeover flag from `ProxyService` state instead of disk) updated by the mutation paths; rebuild only the affected submenu (`Submenu::set_text`/`remove`/`append`); replace `block_on` with values already cached in `ProxyState`.
- **Acceptance:** Log the rebuild duration; typical rebuild < 5 ms and zero `lock_conn!`/file reads in the common path; no `futures::executor::block_on` left in `tray.rs`.

### PERF-17: Single-connection design means long maintenance windows (rollup, backfill, session sync, backup) block the proxy logger and all UI commands

**Type:** refactor · **Priority:** P1 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/database/dao/usage_rollup.rs:62-114` holds `lock_conn!` across the count, the full-table `backfill_missing_usage_costs_on_conn` (84; `usage_stats.rs:1730-1762` scans `WHERE CAST(total_cost_usd AS REAL) <= 0`, no index) and the SAVEPOINT-wrapped aggregation; `database/backup.rs:289` runs `PRAGMA incremental_vacuum` under the lock; `services/session_usage.rs:410-520` holds it per insert for the whole import; `database/mod.rs:79-81` has exactly one `Connection`, so SQLite's WAL reader/writer concurrency cannot be used at all.
- **Problem:** Every one of these windows makes `UsageLogger::log_request` (and thus the Tokio workers that call it, PERF-03) and every synchronous command (PERF-12) wait on an unfair std mutex with no timeout.
- **Proposal:** After PERF-02, split into one writer connection owned by the ingestion task and a small pool (2-4) of read-only connections (`SQLITE_OPEN_READ_ONLY`, `query_only=1`) for UI/dashboard/tray reads; maintenance runs on the writer in short transactions (chunked `LIMIT`ed backfill). This is the main database refactor for the next phase.
- **Acceptance:** During `rollup_and_prune` on a 1M-row DB, dashboard summary still answers in < 100 ms and proxy log latency stays < 200 ms (measured via log timestamps); `Database` exposes `read()`/`write()` accessors and `lock_conn!` is gone from the proxy crate.

### PERF-18: Streaming converters build a `serde_json::Value` per event, serialize to `String`, then `format!` the SSE frame into a second `String` (2-3 copies per token delta)

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/providers/streaming.rs:194-204, 263-265, 291-293, 307-309, 322-324, 337-339, 353-355, 368-370, 476-478, 491-493, 505-507, 536-538, 585-587, 598-600, 613-615, 639-641, 654-...` each do `format!("event: ...\ndata: {}\n\n", serde_json::to_string(&event).unwrap_or_default())` then `Bytes::from(sse_data)`; the `event` is a `json!` macro `Value` built per delta. Identical pattern in `streaming_responses.rs` (2197 lines), `streaming_codex_chat.rs` (1256), `streaming_codex_anthropic.rs` (1194), `streaming_gemini.rs` (1054).
- **Problem:** Per content delta: one `Value` tree allocation, one JSON string allocation, one `format!` allocation (copy), then the frame; combined with PERF-06 this dominates per-token CPU on transform paths (OpenAI/Responses/Gemini-backed Claude providers).
- **Proposal:** Introduce `SseWriter` with a reusable `BytesMut`: `write!(buf, "event: {name}\ndata: ")`, `serde_json::to_writer(&mut buf, &typed_event)`, `buf.extend_from_slice(b"\n\n")`, yield `buf.split().freeze()`; use `#[derive(Serialize)]` structs for the Anthropic event shapes instead of `json!`.
- **Acceptance:** criterion bench converting a recorded 2k-event OpenAI stream to Anthropic SSE shows >= 2x throughput and >= 50 % fewer allocations; output byte-identical (snapshot test).

### PERF-19: All four locale bundles (660 KB raw JSON) are statically imported and shipped in the main chunk

**Type:** performance · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/i18n/index.ts:4-7` imports `en.json` (159,819 B), `ja.json` (198,471 B), `zh.json` (153,384 B), `zh-TW.json` (153,158 B) and registers all of them as `resources` (64-77); the active language is chosen at runtime (13-62).
- **Problem:** ~500 KB of unused translation JSON is parsed on every start (three of four languages are never used in a session).
- **Proposal:** Load the active language via dynamic `import()` (Vite emits a chunk per JSON) with `i18next-resources-to-backend`, keep `en` as the eager fallback.
- **Acceptance:** Main chunk shrinks by >= 400 KB pre-gzip; language switch still works offline (chunk prefetch on settings open).

### PERF-20: Provider cards are not memoized and receive a new inline callback every render while the parent re-renders every 2 s

**Type:** performance · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src/components/providers/ProviderCard.tsx:138` exports a plain function (no `React.memo`); `src/components/providers/ProviderList.tsx:596-654` `SortableProviderCard` wraps each card in `useSortable` and passes `onConfigureUsage ? (item) => onConfigureUsage(item) : () => undefined` (line 630), a fresh closure per render; `ProviderList.tsx:372-442` renders every provider inside one `DndContext`; the list re-renders whenever `useProxyStatus` (2 s, `App.tsx:270`) or `useProvidersQuery` (10 s, `App.tsx:279`) resolve. No virtualization for the provider list.
- **Problem:** With 30-80 providers (power users with several relays per app) every poll re-renders every card including dnd-kit transform computation and lucide icon trees, keeping the webview busy and the fans on.
- **Proposal:** `React.memo(ProviderCard)` with stable callbacks (`useCallback` already exists for most handlers), hoist the `onConfigureUsage` wrapper, pass primitive props instead of the whole `providers` map where possible; virtualize with `@tanstack/react-virtual` (already a dependency) above ~30 items.
- **Acceptance:** React Profiler shows zero `ProviderCard` commits on a proxy-status poll when nothing changed; render time of a 60-card list < 16 ms.

### PERF-21: Recharts trend chart re-renders with full data on every 30 s refetch/event and defines its tooltip component inline

**Type:** performance · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src/components/usage/UsageTrendChart.tsx:129-239` renders `ResponsiveContainer` + `AreaChart` with default animations; `CustomTooltip` is declared inside the component body (line 90), so its identity changes every render and the tooltip subtree remounts; data arrives from `useUsageTrends` (30 s polling + event invalidation, PERF-14) with no memoization or downsampling; `UsageDashboard.tsx:203, 370-406` wraps the dashboard in `framer-motion` `motion.div`s.
- **Problem:** Each invalidation re-animates and re-lays out the SVG chart (hundreds of path nodes for a 90-day range) on the main thread, visibly janking the dashboard while the proxy is active.
- **Proposal:** Hoist `CustomTooltip`, wrap the chart in `React.memo` keyed on a stable data reference, set `isAnimationActive={false}` after first paint, and downsample to <= 180 points for long ranges (aggregate by week when the range exceeds ~120 days).
- **Acceptance:** React Profiler commit count for `UsageTrendChart` equals the number of actual data changes; dashboard interaction frame time < 16 ms during a 1 req/s load.

### PERF-22: Session manager rescans every session root on each open and parses whole transcripts into `serde_json::Value`; search index rebuilt with the most expensive tokenizer

**Type:** performance · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/commands/session_manager.rs:6-11` `list_sessions` always calls `session_manager::scan_sessions` (`session_manager/mod.rs:58-94`), which for Claude (`providers/claude.rs:17-30, 268-286`) walks `~/.claude/projects` recursively and `parse_session` (123-128) opens every `.jsonl` and reads head/tail (`providers/utils.rs:13-49`: files < 16 KB are read entirely, larger ones opened twice) — no mtime/size index or cache; `claude.rs:32-86` `load_messages` parses every line into a `Value` and builds the full `Vec<SessionMessage>` (Claude transcripts commonly reach 50-200 MB). Frontend: `src/hooks/useSessionSearch.ts:27-48` rebuilds a FlexSearch index with `tokenize: "full"` (all substrings) whenever `sessions` changes; message list is virtualized (`SessionManagerPage.tsx:331-337`), the session list is not.
- **Problem:** Opening the Sessions page costs O(total transcript files) syscalls and JSON parsing every time (seconds for thousands of sessions), selecting a large session materializes hundreds of MB, and typing re-tokenizes everything.
- **Proposal:** Persist a `session_index` table keyed by `(provider, path)` with `(mtime, size, meta...)`; on open only stat files and re-parse changed ones (reuse the `session_log_sync` pattern); stream `load_messages` with a `(offset, limit)` API returning the last N messages first and parsing with a typed struct instead of `Value`; use FlexSearch `tokenize: "forward"` and update the index incrementally (`index.add/remove` by key).
- **Acceptance:** Second open of the Sessions page with 3,000 unchanged transcripts completes in < 200 ms (command timing); selecting a 100 MB session shows the tail within 300 ms and peak RSS growth < 50 MB.

### PERF-23: Request-log listing uses OFFSET pagination with a COUNT(*) join per page and performs cost backfill writes inside the read path

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/usage_stats.rs:1512-1520` runs `SELECT COUNT(*) FROM proxy_request_logs l LEFT JOIN providers p ...` on every page, `1530-1548` pages with `ORDER BY l.created_at DESC LIMIT ? OFFSET ?`, and `1551-1557` calls `maybe_backfill_log_costs` per row (1783-1850), which can `UPDATE` rows during a read; the list is refetched every 30 s and on every usage event (PERF-14).
- **Problem:** Deep pages scan and discard `page*page_size` rows; the count query is a full index scan; writes during reads lengthen the mutex hold time and produce `update_hook` sync notifications (`database/mod.rs:83-92`).
- **Proposal:** Keyset pagination on `(created_at, request_id)`, compute the count once per filter change (or `SELECT COUNT(*)` only when `page == 0` and cache it client-side), and remove per-row backfill from reads (the startup backfill at `lib.rs:1105-1108` plus the pre-prune backfill already cover it).
- **Acceptance:** `EXPLAIN QUERY PLAN` shows an index range scan without OFFSET; page 50 of a 200k-row log answers in < 5 ms; no `UPDATE` statements observed while scrolling.

### PERF-24: Startup runs rollup+prune, full-table cost backfill, pricing seed, stream-check cleanup and `incremental_vacuum` synchronously inside `Database::init`

**Type:** performance · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/database/mod.rs:125-162` — `create_tables`, `apply_schema_migrations`, `ensure_incremental_auto_vacuum` (which may `VACUUM` the whole file, 220-232), `ensure_model_pricing_seeded` (145; `schema.rs:2746-2755` re-runs `INSERT OR IGNORE` for every built-in model and a repair pass on every start), `cleanup_old_stream_check_logs(7)` (148), `rollup_and_prune(30)` (151; which first runs the unindexed backfill scan, `usage_rollup.rs:84`), and `PRAGMA incremental_vacuum` (155-159); all before `setup` continues (lib.rs:453) and before the window is shown (PERF-11).
- **Problem:** On a multi-hundred-MB DB these steps take seconds of disk I/O on the critical startup path, with the user staring at nothing.
- **Proposal:** Keep only `create_tables`/migrations synchronous; move seeding, cleanup, rollup and vacuum into the post-start background task (`lib.rs:1042+`) with `spawn_blocking`, and make `rollup_and_prune` skip the backfill when `COUNT(*) WHERE created_at < cutoff AND CAST(total_cost_usd AS REAL) <= 0` is zero (add a partial index `WHERE total_cost_usd = '0'` if backfill stays).
- **Acceptance:** `Database::init` duration (logged) < 100 ms on a 500 MB DB; maintenance runs within 30 s after the window appears.

### PERF-25: Cargo.lock contains two complete HTTP stacks and many duplicated crates (750 packages)

**Type:** tech-debt · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/Cargo.lock` has `reqwest 0.12.28` (app, line 4419) and `reqwest 0.13.2` (line 4466, required by `tauri` and `tauri-plugin-updater`), `brotli 7.0.0 + 8.0.2`, `webpki-roots 0.26.11 + 1.0.6`, `tower 0.4.13 + 0.5.3`, `tower-http 0.5.2 + 0.6.8`, `zip 2.4.2 + 4.6.1`, `thiserror 1 + 2`, `syn 1 + 2`, `windows-sys` x6 (0.45-0.61), `windows-targets` x4, `hashbrown` x4, `phf*` x4, `toml_edit` x4, `getrandom` x4, `rand`/`rand_core`/`rand_chacha` x3, `dirs` x3 (4/5/6), `schemars` x3, `html5ever`/`markup5ever`/`selectors`/`cssparser` x2, `objc2*` x2, `indexmap 1 + 2`, `bitflags 1 + 2`, `base64 0.21 + 0.22`; `Cargo.toml:42, 44-46, 52-53, 63, 69` pin the older majors.
- **Problem:** Duplicate crates inflate the binary (two copies of hyper/reqwest/rustls glue, two brotli decoders), lengthen cold builds (~750 crates, `codegen-units = 1`, thin LTO) and double the surface for CVE tracking.
- **Proposal:** Bump `reqwest` to 0.13 (align with Tauri), `tower-http` 0.6/`tower` 0.5, `zip` 4, `dirs` 6, `brotli` 8, `webpki-roots` 1.0, `thiserror` 2 across the tree; add `cargo tree -d` (or `cargo deny`'s `bans.multiple-versions = "warn"`) to CI with a duplicate budget.
- **Acceptance:** `cargo tree -d | grep -c '^[a-z]'` reduced by >= 40 %; release binary size reported in CI drops measurably (track with `ls -l` in the release workflow).

### PERF-26: Feature and profile choices trade proxy throughput for size: `hyper = full`, two root-certificate stores built separately, `opt-level = "s"` for the whole crate

**Type:** tech-debt · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/Cargo.toml:54` `hyper = { features = ["full"] }` (only http1 client/server are used); `63-64` both `webpki-roots` and `rustls-native-certs`; `hyper_client.rs:59-70` builds the pooled client with webpki roots only while `564-580` builds the raw-path TLS connector with webpki + native certs (inconsistent trust stores, roots parsed twice, ~150 certs each); `Cargo.toml:104-110` `opt-level = "s"`, `lto = "thin"`, `codegen-units = 1` apply to the SSE/JSON hot path; `44-46` ship gzip+brotli+zstd decoders (needed for passthrough); `66` embeds `rquickjs` for usage scripts.
- **Problem:** `opt-level = "s"` disables many vectorized/inlined paths in `serde_json`, `rustls` and `memchr`, costing proxy CPU for a few hundred KB of binary; two trust stores can give different verification results and waste startup CPU on first use.
- **Proposal:** Build one `Arc<rustls::ClientConfig>` at startup and share it between hyper-rustls and the raw connector; set `hyper` features to `["http1", "client", "server"]`; benchmark `opt-level = 3` (or `[profile.release.package.cc-switch] opt-level = 3` while keeping deps at `s`) against the criterion suite and the AppImage size budget; consider `lto = "fat"` if build time allows.
- **Acceptance:** criterion SSE/transform benches compare `s` vs `3` with numbers in the PR; binary size delta documented; a single `global_tls_config()` function referenced from both client paths.

### PERF-27: `serde_json` `preserve_order` turns every parsed object into an `IndexMap`, inflating memory and clone cost on the proxy hot path

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/Cargo.toml:26` `serde_json = { features = ["preserve_order"] }`; request bodies are parsed to `Value` at `handlers.rs:174-175, 705-706, 795-796, 910-911, 1736-1737`, and cloned/rebuilt repeatedly (PERF-04); `forwarder.rs:1527-1535` then canonicalizes (sorts) keys anyway before sending, so insertion order is not what reaches the upstream.
- **Problem:** IndexMap entries carry an extra index vector and hash table per object; for conversation bodies with thousands of small content objects this is ~1.5-2x the memory of `BTreeMap` and slower to clone/drop.
- **Proposal:** Measure first (bench with and without the feature on a 1 MB Claude body); if order matters only for config files written back to disk, keep `preserve_order` for those paths via a separate `serde_json` alias is not possible, so instead parse request bodies into typed structs with `Box<RawValue>` for passthrough fields (`messages`, `tools`, `system`) so they are never materialized.
- **Acceptance:** Bench shows parse+clone+serialize of a 1 MB body improved by >= 30 %; snapshot tests confirm wire-identical output.

### PERF-28: Debug-level logging formats and writes every request body and every SSE event to a 1 GB log file

**Type:** performance · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/forwarder.rs:2085-2092` logs the full serialized request body at debug; `response_processor.rs:216-220` logs the full non-streaming response body; `response_processor.rs:767-770` logs every SSE `data:` payload; `response_processor.rs:690-691` forces SSE inspection when debug is enabled; the log plugin is initialized at `Trace` with a 1 GB file cap and stdout target (`lib.rs:338-368`), and the effective level comes from the DB (`lib.rs:968-979`).
- **Problem:** Users turn on debug precisely when the proxy misbehaves; at that moment every token is formatted twice and written synchronously through `tauri-plugin-log`, so throughput collapses and the log grows by MBs per minute, making the captured evidence less useful.
- **Proposal:** Move bodies and per-event payloads to `trace`, cap them with `summarize_text_for_log` (already present at `forwarder.rs:3197`), and add an in-memory ring buffer exposed to the UI for recent request diagnostics; keep the file target at `info` by default.
- **Acceptance:** With debug enabled, `oha` throughput on a passthrough stream degrades by < 20 % versus info level; log growth < 100 KB per request.

### PERF-29: Frontend dependency weight: framer-motion for simple fades, 14 Radix packages, recharts 3, CodeMirror 6 with four language packs, flexsearch, cmdk; no bundle analysis in CI

**Type:** tech-debt · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `package.json:43-93` lists `@codemirror/*` x7 (+ `codemirror`), `@radix-ui/*` x14, `recharts ^3.5.1`, `framer-motion ^12`, `flexsearch`, `cmdk`, `lucide-react`, `i18next`/`react-i18next`, `@lobehub/icons-static-svg` (only used at build time by `scripts/extract-icons.js`); `framer-motion` is imported in 11 files mostly for fade/slide (`ProviderList.tsx:462-524`, `UsageDashboard.tsx:203`, `App.tsx:3`), while equivalent CSS keyframes already exist in `tailwind.config.cjs:231-295`; `.github/workflows/ci.yml` has no bundle report.
- **Problem:** Each of these libraries ships 50-450 KB of JS; without code splitting (PERF-15) they are all parsed at boot even though the chart, editors and search are used on secondary pages.
- **Proposal:** Replace framer-motion fades with Tailwind animations (keep it only where layout animations are needed), move `@lobehub/icons-static-svg` to `devDependencies`, lazy-load recharts/CodeMirror/flexsearch with their pages, and add `rollup-plugin-visualizer` + a `size-limit`-style check to CI.
- **Acceptance:** Visualizer report checked into CI artifacts; total JS shipped at boot reduced by >= 40 %.

### PERF-30: No performance measurement infrastructure (benchmarks, load harness, startup spans, bundle/DB plan checks)

**Type:** test · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `ls src-tauri/benches` -> none; no `criterion`/`divan` in `src-tauri/Cargo.toml:112-114` dev-dependencies; `.github/workflows/ci.yml:1-80` runs typecheck, prettier, vitest and cargo checks only; no `Instant` spans in `lib.rs` setup; no `EXPLAIN QUERY PLAN` tests in `database/tests.rs`; no k6/oha scripts under `tests/` (which holds 68 vitest suites).
- **Problem:** None of the findings above can be prioritized or regression-guarded with numbers today.
- **Proposal:** (1) criterion benches in `src-tauri/benches/`: `sse_passthrough`, `anthropic_to_openai`/`openai_to_anthropic` transforms, `streaming_convert` on recorded fixtures, `prepare_request_body`, `usage_parse`; (2) a proxy load harness: a tiny Rust/`msw`-style mock upstream that replays recorded SSE at a configurable rate plus `oha`/`k6` scripts (`k6 run --vus 16 --duration 60s`) measuring TTFB p50/p95, throughput, RSS (`/proc/self/status`) and Tokio worker busy time; (3) startup spans logged per phase (PERF-11) and a `startup_ms` metric in About; (4) `rollup-plugin-visualizer` artifact + size budget in CI; (5) DB plan tests: unit tests asserting `EXPLAIN QUERY PLAN` output for the summary/trends/logs/limits SQL contains no `SCAN` on `proxy_request_logs` without an index and no correlated subquery; (6) a synthetic DB generator (`cargo run --bin gen-usage-db -- --rows 500000`) for dashboard timing.
- **Acceptance:** `cargo bench` and `k6` jobs run nightly with results posted as workflow artifacts; PRs touching `proxy/` or `database/` show before/after numbers.

### PERF-31: No SQLite statement cache: 310 `conn.execute`/`prepare` call sites, zero `prepare_cached`

**Type:** performance · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `rg prepare_cached src-tauri/src` returns nothing; `rg "conn.execute\(|\.prepare\("` returns 310 sites, including the request-path statements in `proxy/usage/logger.rs:81-118`, `dao/proxy.rs:127-131, 173-177, 222-243`, `dao/providers.rs:136-139`, and the ~1 KB dedup SQL built with `format!` per call in `usage_stats.rs:296-330`.
- **Problem:** SQLite re-tokenizes and plans each statement on every execution (tens of microseconds each, more for the multi-KB dashboard SQL), and the `format!`-built SQL strings defeat any cache.
- **Proposal:** Enable `Connection::set_prepared_statement_cache_capacity(64)` and switch hot statements to `prepare_cached`; hoist the dedup filter into `static` SQL strings (`LazyLock<String>`) with parameterized aliases.
- **Acceptance:** A micro-bench of 10k `get_proxy_config_for_app` calls improves by >= 30 %; `sqlite3_stmt` prepare count (via `sqlite3_status`/trace callback) stays flat during a proxy load test.

### PERF-33: Frontend boot waterfall: render blocked on `get_init_error`, then ~10 independent invokes including a duplicated env-conflict scan

**Type:** performance · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/main.tsx:76-103` awaits `invoke("get_init_error")` before `createRoot().render()`; on mount `App.tsx` fires `useSettingsQuery` (188), `useProxyStatus` (270, two invokes), `useProvidersQuery` (279, two invokes), window maximized sync (448-476), `checkAllEnvConflicts` (493-515; backend reads shell profile files), `get_migration_result` (517-533), `get_skills_migration_result` (535-561), and `checkEnvConflicts(activeApp)` again (563-593, a subset of the all-apps scan that just ran).
- **Problem:** First paint waits a full IPC round trip, then the UI fans out 10+ commands (several synchronous on the main thread, PERF-12) that contend with the backend's own startup tasks (session sync, PERF-10) for the DB mutex.
- **Proposal:** Render the shell immediately and show the DB-upgrade screen if/when `get_init_error` resolves with an error; add one `get_bootstrap_state` command returning settings + providers(current app) + proxy overview + migration results in a single call; skip the per-app env check when the all-apps result is fresh.
- **Acceptance:** Webview `performance.mark('first-provider-card')` < 400 ms after `DOMContentLoaded` on the Linux CI runner; invoke count during the first second <= 5.

### PERF-32: Global `invalidateQueries()` with no key refetches every mounted query (8 sites) and 164 invalidation sites in total

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/settings/WebdavSyncSection.tsx:517, 580, 640, 736, 796, 851, 909` and `src/hooks/useBackupManager.ts:25` call `queryClient.invalidateQueries()` without a key; `rg -c invalidateQueries src` totals 164 sites (mutations.ts 27, proxy.ts 22, failover.ts 17, App.tsx 15, ...).
- **Problem:** After a sync/backup the app refetches dozens of queries at once (providers for all apps, usage, settings, skills, MCP), spiking the DB mutex and IPC; the broad invalidations in `App.tsx` run on every provider switch.
- **Proposal:** Scope invalidations to the affected namespaces (`["providers"]`, `["settings"]`, `usageKeys.all`) and prefer `setQueryData` from mutation responses where the backend already returns the new row.
- **Acceptance:** A WebDAV sync triggers <= 10 refetches (React Query devtools); provider switch triggers <= 3.

### PERF-34: Each accepted connection allocates an 8 KB peek buffer and re-parses the request head; request bodies are fully buffered with a 200 MB ceiling

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/server.rs:159-175` allocates `vec![0u8; 8192]` and runs `httparse` with 128 header slots per connection before hyper parses the same bytes; `178-192` clones the `OriginalHeaderCases` (`Vec<(String, Vec<u8>)>`) for every request on the connection; `server.rs:368` sets `DefaultBodyLimit::max(200 MB)` and every handler collects the whole body (`handlers.rs:168-176, 700-706, 790-796, 905-911, 1731-1737`) before parsing.
- **Problem:** Minor CPU/alloc overhead per connection, but the 200 MB limit means a misbehaving local client can make the proxy hold 200 MB per in-flight request (times `oha -c N`), and memory is not reclaimed until the response completes.
- **Proposal:** Reuse a thread-local peek buffer, wrap `OriginalHeaderCases` in `Arc`, and lower the body limit to what Gemini file uploads actually need (or make it configurable, default 32 MB).
- **Acceptance:** RSS under `oha -c 64` with 1 MB bodies stays < 200 MB; no functional change in header-case tests.

### PERF-35: `notify_log_recorded` spawns an OS thread per debounce window and the DB `update_hook` runs for every row change

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/usage_events.rs:46-68` does `std::thread::spawn(|| { sleep(200ms); emit })` whenever no emit is scheduled; it is called per proxy log (`logger.rs:121`), per imported session row (`session_usage.rs:514-516`) and after rollup; `database/mod.rs:83-92` registers an `update_hook` that calls `webdav_auto_sync::notify_db_changed` and `s3_auto_sync::notify_db_changed` for every INSERT/UPDATE/DELETE (including the thousands of session-sync inserts).
- **Problem:** Up to five short-lived OS threads per second during bursts (thread creation ~50-100 us plus stack mmap), and hook overhead on bulk inserts.
- **Proposal:** Use a single long-lived notifier task (`tokio::spawn` with `tokio::time::sleep`) or `tauri::async_runtime::spawn`; suppress the update hook inside bulk import transactions (the auto-sync modules already expose `is_auto_sync_suppressed`).
- **Acceptance:** No `std::thread::spawn` in `usage_events.rs`; thread count (`/proc/self/status Threads`) stays constant during a 1 req/s load.

### PERF-36: Six indexes (incl. three single-column ones) and a TEXT UUID primary key on the append-only `proxy_request_logs` table add write amplification

**Type:** performance · **Priority:** P3 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/database/schema.rs:197-232` — `request_id TEXT PRIMARY KEY` (random UUID/`session:msg_...` strings, so B-tree inserts are random), plus `idx_request_logs_provider`, `_created_at`, `_model`, `_session`, `_status`, `_app_created_at` (2783) and the 7-column expression index `idx_request_logs_dedup_lookup_expr` (2812-2820). Queries examined (`usage_stats.rs:596-1557`) filter by `(app_type, created_at)`, `provider_id`, `request_id` and the dedup tuple; `status_code`-only and `model`-only lookups do not appear.
- **Problem:** Every insert updates 8 B-trees; the random TEXT PK and `INSERT OR REPLACE` (`logger.rs:82`) cause page splits and extra fsyncs under the default journal mode (PERF-02).
- **Proposal:** After PERF-02/08, drop `idx_request_logs_model`, `_status` and `_session` unless a query needs them, and consider `id INTEGER PRIMARY KEY` + `UNIQUE(request_id)`; verify with `EXPLAIN QUERY PLAN` across all usage SQL.
- **Acceptance:** Insert bench (PERF-02) improves further by >= 20 %; all plan tests (PERF-30) still use an index.

### PERF-37: Non-streaming responses are always fully buffered, cloned and decompressed on the proxy even when no parsing or transform is needed

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/response_processor.rs:80-127` `read_decoded_body` collects the body, `raw_bytes.clone()` (cheap refcount) and decompresses when `content-encoding` is set, then re-sends the larger plaintext to the local client; `handle_non_streaming` (208-320) parses the whole body to `Value` for usage when logging is enabled; `forwarder.rs:2234-2262` additionally buffers the whole body a second time in `prepare_success_response_for_failover` whenever `non_streaming_timeout > 0`.
- **Problem:** For large non-streaming JSON (e.g. tool-heavy responses) the proxy pays decompression CPU and memory the client would have handled itself; double buffering holds two copies until the response is written.
- **Proposal:** Decompress only when usage parsing/transform needs the plaintext; otherwise pass through bytes with the original `content-encoding`; make the failover buffering reuse the already-collected `Bytes` instead of re-collecting.
- **Acceptance:** Passthrough of a 5 MB gzip JSON response shows no `decompress_body` call in a trace and RSS increase bounded by one body copy.

### PERF-38: `CodexChatHistoryStore` caps entries (512 responses) but not bytes, and `GeminiShadowStore` defaults are not surfaced for tuning

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** low

- **Evidence:** `src-tauri/src/proxy/providers/codex_chat_history.rs:10` `MAX_CACHED_RESPONSES = 512`, entries hold full tool-call `Value`s (`calls_by_id: HashMap<String, Value>`, line 14) with no TTL or byte budget (eviction only at 234-240 by count); `gemini_shadow.rs:111-133` has `max_sessions`/`max_turns_per_session` with LRU pruning (good) but is constructed with `Default` at `server.rs:80`.
- **Problem:** Long-running proxy processes with large tool outputs can hold hundreds of MB in the Codex history cache; there is no visibility into its size.
- **Proposal:** Track approximate byte size per entry and evict by bytes (e.g. 32 MB) plus a 30-minute TTL; expose counts/bytes on `/status` for monitoring.
- **Acceptance:** Soak test (k6, 2 h, tool-heavy fixtures) shows flat RSS after warm-up; `/status` reports cache bytes.

### PERF-39: `scan_unmanaged_skills` walks 8+ directories and reads every `SKILL.md` synchronously on the main thread when the Skills page opens

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/commands/skill.rs:102-106` is a sync `pub fn` command; `services/skill.rs:1377-1410+` collects skill dirs for every `AppType`, the agents dir and the SSOT dir, `read_dir`s each and parses metadata; the frontend query (`src/hooks/useSkills.ts:21-60`) has `staleTime: Infinity` (good) but the first scan per session still blocks.
- **Problem:** With hundreds of installed skills (common for power users) the UI freezes for the duration of the scan.
- **Proposal:** Make the command `async` + `spawn_blocking`; cache results keyed by directory mtimes.
- **Acceptance:** Skills page open with 500 skills shows no main-thread task > 50 ms; second open is served from cache in < 5 ms.

### PERF-40: `ProviderRouter::select_providers` allocates a circuit-breaker key string and takes an async RwLock per provider per request even when failover is disabled for all but one provider

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/provider_router.rs:65-78` does `format!("{app_type}:{}", provider.id)`, `get_or_create_circuit_breaker(...).await` (read lock on `Arc<RwLock<HashMap>>`, 166-170) and `breaker.is_available().await` per queued provider; `forwarder.rs:433-443` repeats `allow_provider_request` per attempt; `forwarder.rs:356-361` takes `status.write().await` per request for counters.
- **Problem:** Small but on the request path; cumulatively with PERF-05 it is several lock acquisitions per request.
- **Proposal:** Key breakers by `(app_type, provider_id)` `Arc<str>` without formatting, keep `Arc<CircuitBreaker>` references in the cached provider snapshot (PERF-05), and use atomics for `total_requests`/`last_request_at`.
- **Acceptance:** No `format!` or `.await` on locks in `select_providers` for the single-provider path; micro-bench shows < 1 us per call.
