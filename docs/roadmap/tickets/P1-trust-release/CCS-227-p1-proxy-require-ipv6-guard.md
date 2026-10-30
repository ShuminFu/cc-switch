# CCS-227: [proxy-tests] Fail the [::1] proxy tests instead of skipping when CCS_REQUIRE_IPV6 is set

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P1`, `p3-low`, `area:proxy-tests`

> **Ticket key:** `p1-proxy-require-ipv6-guard` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-061). Source tickets: [CCS-061](../P1-trust-release/CCS-061-p1-proxy-connect-ipv6.md).

## Problem
`bind_ipv6_loopback()` in `src-tauri/src/proxy/hyper_client.rs` (~1124) prints "skipping: IPv6 loopback unavailable" and returns None. So `connect_via_proxy_reaches_ipv6_literal_proxy` (~1164) and `send_raw_request_reaches_ipv6_literal_upstream` (~1191) can pass on CI without running, and no run could ever prove CCS-061 AC1-3. This container has no IPv6 (EAFNOSUPPORT), so the real assertions need CI; the guard makes that CI run meaningful.

## Proposed change
- When `std::env::var("CCS_REQUIRE_IPV6")` is set, panic on a bind error instead of returning None.
- Set `CCS_REQUIRE_IPV6: 1` on the Linux and macOS legs of the backend `cargo test` step in `.github/workflows/ci.yml` (~164); Windows at the implementer's discretion.
- Touches `src-tauri/src/proxy/`: add a fixture change or the `no-fixture-needed` label (CCS-034).

## Acceptance criteria / verification
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::hyper_client` still passes here (tests skip).
- [ ] `CCS_REQUIRE_IPV6=1 cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::hyper_client::tests::connect_via_proxy_reaches_ipv6_literal_proxy` panics here with EAFNOSUPPORT.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `~/go/bin/actionlint .github/workflows/ci.yml` pass.

## Dependencies
None.
