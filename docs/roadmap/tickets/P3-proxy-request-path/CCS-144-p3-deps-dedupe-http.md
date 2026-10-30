# CCS-144: [deps] Dedupe tower, tower-http, webpki-roots and reqwest versions

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P3`, `p3-low`, `area:deps`

> **Ticket key:** `p3-deps-dedupe-http` · **Phase:** P3 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p0-unused-tower-http`, `p0-proxy-harness-e2e`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-duplicate-http-stacks, partially real. The app's direct deps (tower 0.4, webpki-roots 0.26, brotli 7, dirs 5, zip 2.x) plus reqwest 0.12 next to tauri-plugin-updater's 0.13 compile 73 to 79 duplicate crates.

## Evidence
- src-tauri/Cargo.toml:39-69; `cargo tree -d`

## Proposed change
Move to tower 0.5 and webpki-roots 1.0. Evaluate reqwest 0.13 (the rustls feature names changed). brotli 8 and dirs 6 are optional.

## Acceptance criteria / verification
- [x] `cargo tree -d -e normal --depth 0 | sort -u | wc -l` drops; `cargo tree -i reqwest` shows one version if migrated
- [x] proxy_e2e and the golden tests pass; release binary size and clean build time compared in the PR

## Dependencies
p0-unused-tower-http, p0-proxy-harness-e2e

## Implementation notes

**Summary.** tower was already on 0.5 (and tower-http already removed) by the
time this ticket ran, so the change covers the remaining duplicates:

- `reqwest` 0.12 -> 0.13, so the app and `tauri-plugin-updater` share one
  reqwest. Built with `default-features = false` and
  `rustls-no-provider, charset, http2, system-proxy, json, stream, socks, query, form`
  (`query`/`form` are opt-in features in 0.13). `rustls-no-provider` makes reqwest
  reuse the process-level ring provider (the one `hyper_client` and `lib.rs` already use)
  instead of linking aws-lc-rs. Certificate verification now goes through
  `rustls-platform-verifier` (the OS trust store), which matches what the old default
  native-tls backend did. native-tls, hyper-tls and openssl are gone from the tree.
- New `proxy::http_client::ensure_tls_provider()`. It is called before every reqwest
  client build (`build_client`, the Hermes probe, the global-proxy test command) because
  with no provider installed, reqwest panics.
- `webpki-roots` 0.26 -> 1.0 (no code change, since `TLS_SERVER_ROOTS` is the same API),
  `brotli` 7 -> 8 (no code change; this matches tauri-utils), `dirs` 5 -> 6 (no code change).
  `dirs` 4 is still pulled in by `auto-launch` 0.5.

**Files.** `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`,
`src-tauri/src/proxy/http_client.rs` (helper + test `test_build_client_uses_ring_provider`),
`src-tauri/src/commands/hermes.rs`, `src-tauri/src/commands/global_proxy.rs`.

**Commits.** 06775d0 (code), plus the follow-up commit that records these notes.

**Verification (Linux).**
- `cargo tree -d -e normal --depth 0 | sort -u | wc -l`: 70 -> 61. `Cargo.lock` packages: 766 -> 743.
- `cargo tree -i reqwest -e normal`: only `reqwest v0.13.2` (cc-switch + tauri-plugin-updater).
- Clean `cargo build --release --locked` (Linux x86_64, 4 cores, separate empty target dirs, same machine):
  - before: 803 s, binary 27,875,872 B
  - after: 728 s, binary 26,848,608 B (-1.03 MB, -3.7%)
  - A first attempt that kept reqwest's default `rustls` feature (aws-lc-rs provider)
    produced 29,323,616 B (+5.2%). That is why `rustls-no-provider` is used.
- `cargo fmt --check`: clean. `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --no-fail-fast`: 2550 passed / 0 failed.
- `cargo test --features testkit --test proxy_e2e --test proxy_golden --test proxy_capture --test proxy_trace`:
  3 + 27 + 6 + 3 passed, 0 failed.
- Not checked locally: TLS behaviour on macOS and Windows (platform verifier backends, system proxy
  detection through hyper-util `client-proxy-system`). These only compile/run on those runners.
