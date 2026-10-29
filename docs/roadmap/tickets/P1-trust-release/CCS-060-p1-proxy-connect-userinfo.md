# CCS-060: [proxy] Percent-decode proxy credentials before building Proxy-Authorization

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-connect-userinfo` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-connect-userinfo-decode. url::Url username() and password() return percent-encoded strings, and GlobalProxySettings always encodes special characters, so proxy auth fails for passwords that contain @ : # % / ? or non-ASCII.

## Evidence
- src-tauri/src/proxy/hyper_client.rs:465-470; src/components/settings/GlobalProxySettings.tsx:50-66

## Proposed change
Extract `fn proxy_auth_header(&Url) -> Option<String>` that runs percent_decode_str on both parts (add percent-encoding as an explicit dependency).

## Acceptance criteria / verification
- [x] Test: `http://us%40er:p%40ss%3Aw0rd@127.0.0.1:8080` gives `Basic base64(us@er:p@ss:w0rd)`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::hyper_client`

## Dependencies
None.

## Implementation notes
- **Summary:** Extracted `fn proxy_auth_header(&url::Url) -> Option<String>` in `src-tauri/src/proxy/hyper_client.rs`. It percent-decodes `username()` and `password()` with `percent_encoding::percent_decode_str` (raw bytes, so non-ASCII UTF-8 is preserved exactly), joins them with `:`, and returns `Basic <base64>`; `connect_via_proxy` now uses it for the `Proxy-Authorization` line. `percent-encoding = "2.3"` added as an explicit dependency (already in the lockfile transitively, 2.3.2).
- **Tests:** `proxy_auth_header_percent_decodes_userinfo` (ticket case `http://us%40er:p%40ss%3Aw0rd@127.0.0.1:8080` -> `Basic base64(us@er:p@ss:w0rd)`, plus `# % / ?` and non-ASCII cases; the old code base64-encoded the still-encoded string, so this fails before the fix) and `proxy_auth_header_handles_missing_parts` (no userinfo -> `None`, username-only -> `user:`, plain credentials unchanged).
- **Files:** `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/src/proxy/hyper_client.rs`.
- **Commit:** b1d57cb
- **Verification:**
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean
  - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::hyper_client`: 10 passed, 0 failed
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2127 passed, 0 failed, 3 ignored
