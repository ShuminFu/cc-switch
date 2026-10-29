# CCS-058: [proxy] Make mask_url panic-free and never echo credentials from unparseable proxy URLs

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-mask-url` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-mask-url-panic. The fallback `&url[..20]` byte-slices and panics on multibyte input. Malformed URLs such as `http//user:pass@host` leak userinfo into error strings and logs.

## Evidence
- src-tauri/src/proxy/http_client.rs:319-335; build_client :232-233; callers commands/global_proxy.rs:21,65,122,148

## Proposed change
When parsing fails, return a constant `<invalid proxy url>`, or strip everything before the last '@' and truncate with chars().take(20).

## Acceptance criteria / verification
- [x] `mask_url("代理代理代理代理代理代理")` does not panic
- [x] `mask_url("http//user:pass@proxy.example.com")` has no user/pass
- [x] `build_client(Some("http//user:secret@host"))` errors without "secret"
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::http_client`

## Dependencies
None.

## Implementation notes

**Summary.** `mask_url` (src-tauri/src/proxy/http_client.rs) no longer byte-slices
its fallback. When `url::Url::parse` fails, it drops everything up to and including
the last `@` (so any `user:pass@` userinfo is discarded, even when the password
itself contains `@`), then truncates the remainder to 20 *chars* with `chars().take(20)`
and appends `...` when longer. Parseable URLs keep the existing `scheme://host[:port]`
output. All `build_client` errors and the `commands/global_proxy.rs` log callers go
through `mask_url`, so they inherit the fix with no caller changes.

**Files.**
- `src-tauri/src/proxy/http_client.rs` — fallback branch of `mask_url`; new tests
  `test_mask_url_multibyte_does_not_panic`, `test_mask_url_unparseable_strips_userinfo`,
  `test_build_client_invalid_url_does_not_leak_secret`.

**Commits.** `6a05b13` (code, tests and this note).

**Verification.**
- Regression check: with the old fallback restored, the 3 new tests FAIL (multibyte
  slice panic; `user`/`pass`/`secret` present in output); with the fix all pass.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::http_client` — 10 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` — 2120 passed, 0 failed, 3 ignored.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` — clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — clean.
