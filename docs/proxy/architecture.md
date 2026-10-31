# Local proxy architecture

How a request travels through cc-switch's local proxy, how failover and the
circuit breaker decide where it goes, which timeouts apply, and what happens to
headers. Code lives in `src-tauri/src/proxy/`; field mapping between protocols
is in [format-matrix.md](format-matrix.md).

## Request lifecycle

```
client ─► server.rs (accept loop)
            │  connection caps: 1024 total, 64 per non-loopback IP
            │  header read timeout 30 s; per-request header-case capture
            ▼
          access_guard (Host/Origin check, access token for non-local peers,
            │            64 MB body limit)
            ▼
          handlers.rs  (route → RequestContext: app, session id, timeouts)
            ▼
          provider_router.select_providers   (who may be tried, in order)
            ▼
          forwarder.forward_with_retry       (one attempt per provider)
            │  per attempt: budget check → breaker permit → endpoint/URL →
            │  body transform → auth + header policy → send → validate
            ▼
          response_processor / handlers      (passthrough or protocol bridge,
                                              usage logging)
```

The attempt stages are separate modules in `proxy/pipeline/`:
`endpoint.rs` (route and URL), `body.rs` (transform and outbound body policy),
`auth.rs` (credentials, including Copilot / Codex OAuth / Gemini CLI token
refresh), `headers.rs` (header policy), `transport.rs` (the two HTTP stacks),
`validate.rs` (2xx bodies that are really errors), `timeout.rs`, `metrics.rs`.

## Provider selection and failover

`ProviderRouter::select_providers` (`provider_router.rs`):

| Failover | Queue | Candidates |
|---|---|---|
| off | any | the current provider only; the breaker is not consulted |
| on | empty | the current provider (logged as `FO-006`) |
| on | exactly one provider | that provider; the breaker is not consulted (nothing to fail over to) |
| on | two or more | queue order (P1, P2, ...), skipping providers whose breaker is open |

If every candidate's breaker is open the request fails with
`AllProvidersCircuitOpen` (503).

The forwarder tries candidates in order, at most `max_retries + 1` attempts
per request (the per-app `max_retries` setting). Whether
an error moves on to the next provider (`categorize_proxy_error` in
`forwarder.rs`):

- **Next provider:** timeouts, connection failures, stream idle timeouts,
  transform/auth/config errors, upstream 401/403/404/408/409/429/451 and every
  5xx except 501.
- **Stop (client problem, does not count against the breaker):** upstream
  400, 405, 406, 413, 414, 415, 422, 501, and auth errors from the built-in
  official Codex provider.

A 2xx response whose body is an error envelope (checked in
`pipeline/validate.rs`) is treated like an upstream error. Thinking-signature
and thinking-budget rectifiers (`thinking_rectifier.rs`,
`thinking_budget_rectifier.rs`) may retry the *same* provider once with a fixed
body before failing over; the fixed body is carried to the next provider.

## Circuit breaker

One breaker per `app:provider` (`circuit_breaker.rs`), configured per app in
`AppProxyConfig`:

```
Closed ──(failure_threshold consecutive failures, or error rate ≥
          error_rate_threshold over ≥ min_requests recent outcomes)──► Open
Open ──(timeout_seconds elapsed)──► HalfOpen (limited probe permits)
HalfOpen ──(success_threshold successes)──► Closed
HalfOpen ──(any failure)──► Open
```

Streaming responses are recorded when the stream ends, not at the first byte.
Breaker states are exported on `/metrics`.

## Timeouts

See the table at the top of `pipeline/timeout.rs`. In short: connect 10 s
always; with failover **on**, the configured first-byte / idle / non-streaming
timeouts apply; with failover **off** (or a value of 0) only a 600 s ceiling
on response headers applies and streams are not cut.

## Header policy

`pipeline/headers.rs` builds the upstream header map from the client's headers,
keeping their order:

- **Replaced:** `host` (upstream authority), credentials (`authorization`,
  `x-api-key`, `x-goog-api-key`, `api-key`) by the provider's,
  `accept-encoding` (`identity` when the body must be read), `user-agent` when
  the provider sets one, `anthropic-beta` (merged).
- **Dropped:** hop-by-hop headers, `content-length` / `transfer-encoding`,
  forwarding and CDN client-IP headers (`x-forwarded-*`, `forwarded`,
  `cf-*`, `true-client-ip`, ...), tracing headers (`x-request-id`,
  `x-correlation-id`, `traceparent`, `tracestate`, `x-b3-*`, ...), and
  cc-switch's own `x-cc-switch-token`. Dropping tracing headers means a
  client's own trace ids do not reach the upstream.
- Native Anthropic providers are sent through a raw writer that reproduces the
  client's header casing and order for each request.

## Upstream-specific behaviour

Detected from the upstream URL (`providers/upstream_compat.rs`):

- **Azure OpenAI** (`*.openai.azure.com`, `*.cognitiveservices.azure.com`,
  `*.services.ai.azure.com`): the key is sent as `api-key` (Entra ID JWTs stay
  `Authorization: Bearer`). Use "full URL" mode with the deployment URL, e.g.
  `https://res.openai.azure.com/openai/deployments/<deployment>/chat/completions?api-version=2024-10-21`.
- **Mistral** (`api.mistral.ai`, `codestral.mistral.ai`): unknown fields are
  removed, `seed` → `random_seed`, `max_completion_tokens` → `max_tokens`.
- **Ollama** (port 11434): `max_completion_tokens` → `max_tokens`.
- Bedrock and Vertex need cloud credential flows (SigV4, service-account
  OAuth) and are not supported natively yet.

## Observability

- `GET /status`: running state; full counters with the access token.
- `GET /metrics`: Prometheus text format, served to local peers or with the
  access token. Request/success/failure/failover counters, active
  connections, uptime, per-provider attempt counters and time-to-headers
  latency histograms (`ccswitch_provider_latency_seconds`), breaker state
  gauges and the Codex history cache size.
- Requests and attempts run inside `tracing` spans (`proxy_request`,
  `proxy_attempt`); they are no-ops unless a subscriber is installed.
- Logs: at `info` one line per request (query string redacted). `debug` adds
  the request body *shape* only (size, top-level keys, array lengths); body
  excerpts and per-SSE-event payloads are written only at `trace`.

## Network exposure

The proxy listens on loopback by default. When bound to another address,
requests from other hosts need the access token (`Authorization: Bearer` or
`x-cc-switch-token`), and each remote IP is limited to 64 open connections.
