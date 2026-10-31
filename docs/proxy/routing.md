# Proxy routing, body passthrough and trust store

This page covers three request-path behaviours of the local proxy that are not
part of the format matrix: how a provider is chosen (PROXY-19), when the
client's bytes are forwarded unchanged (PROXY-17, PERF-37), and which TLS roots
every upstream client trusts (PROXY-40). The module map is in
[architecture.md](architecture.md).

## Routing (`src-tauri/src/proxy/routing.rs`)

Without a routing config the proxy behaves as before: with failover on, the
providers in the failover queue whose breaker allows a request are tried in
queue order, at most `max_retries + 1` of them.

A per-app routing config can change three things. Every field is optional and
the defaults reproduce the behaviour above.

| Field (JSON)               | Default           | Effect                                                                                                                                                                                                                                                                        |
| -------------------------- | ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `strategy`                 | `"priority"`      | Which available provider is tried first. `"roundRobin"` rotates the first provider per request. `"weighted"` uses smooth weighted round-robin (the nginx algorithm, deterministic) over `weights`. The other providers keep queue order as the failover chain.                |
| `weights`                  | `{}`              | Provider id to weight for `"weighted"`. A missing id has weight 1, weight 0 is never tried first (it can still be a failover target).                                                                                                                                         |
| `stickySessions`           | `false`           | Pin a client-provided session id to the provider that last served it. While that provider is still selectable it is tried first, so a conversation keeps its prompt cache and thinking signatures after an earlier provider recovers. Generated session ids are never pinned. |
| `stickyTtlSecs`            | `0` (= 1800)      | A pin expires this long after its last use. At most 4096 pins are kept (oldest evicted).                                                                                                                                                                                      |
| `maxConcurrentPerProvider` | `0` (= unlimited) | In-flight requests allowed per provider. A provider at its limit is skipped like one with an open breaker; if every provider is at its limit the client gets a 429. The slot is held until the response, including a stream, ends.                                            |

Storage: one JSON object in the `settings` table under the key
`proxy_routing_config`, keyed by app id, for example:

```json
{
  "claude": { "stickySessions": true, "stickyTtlSecs": 3600 },
  "codex": { "strategy": "weighted", "weights": { "p-main": 3, "p-spare": 1 } }
}
```

Commands: `get_proxy_routing_config(appType)` and
`set_proxy_routing_config(appType, config)`. A running proxy reads the config
through a cache that is refreshed at least every 5 seconds, so a change applies
without a restart. Saving the default config removes the app's entry.

Not implemented yet: least-latency routing, token-bucket `rpm`/`tpm` limits
and a global retry budget. The settings UI is also still missing.

## Body passthrough

- **Requests.** The handler keeps the client's (decoded) body bytes in the
  request extensions (`pipeline::inbound::ClientBodyBytes`, a reference-counted
  `Bytes` shared by every attempt). When the final outbound body equals the
  client's parsed body (`serde_json::Value` equality ignores object key order),
  those bytes are sent upstream as they are: no re-serialization, and the wire
  bytes match what the client sent. Any change re-serializes the body in
  canonical key order as before. Changes include model mapping, private `_`
  keys, a format transform, a rectifier retry, optimizers and local overrides.
- **Non-streaming responses.** A passthrough response reaches the client as the
  upstream sent it, with its `content-encoding` (the client's own
  `accept-encoding` was forwarded). The proxy decompresses a temporary copy only
  when usage logging or trace logging needs the plaintext. Transform paths
  still decode, because they must parse the body.

## Trust store (`src-tauri/src/proxy/tls_roots.rs`)

The pooled reqwest client, the hyper-util fallback client and the raw
header-case path trust the same roots:

1. the Mozilla/webpki roots,
2. the OS certificate store (install a corporate or TLS-intercepting proxy CA
   there), and
3. an optional extra CA bundle in PEM format, read from the path in the
   `CC_SWITCH_EXTRA_CA_FILE` environment variable or, when that is unset, from
   `~/.cc-switch/extra-ca.pem` if the file exists.

The bundle is read once at startup, so restart the app after you change it.
Entries that do not parse are skipped and logged at `warn` level. There is no
option to skip TLS verification.
