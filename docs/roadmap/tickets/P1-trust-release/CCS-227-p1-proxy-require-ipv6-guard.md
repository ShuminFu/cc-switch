# CCS-227: [proxy-tests] Fail the [::1] proxy tests instead of skipping when CCS_REQUIRE_IPV6 is set

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P1`, `p3-low`, `area:proxy-tests`

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
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::hyper_client` still passes here (tests skip).
- [x] `CCS_REQUIRE_IPV6=1 cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::hyper_client::tests::connect_via_proxy_reaches_ipv6_literal_proxy` panics here with EAFNOSUPPORT.
- [x] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `~/go/bin/actionlint .github/workflows/ci.yml` pass.

## Dependencies
None.

## Implementation notes
- `src-tauri/src/proxy/hyper_client.rs` (tests module): `bind_ipv6_loopback()` panics with the bind error when `CCS_REQUIRE_IPV6` is set to a non-empty value other than `0`; otherwise it still prints the skip line and returns `None`. The env check is a small pure helper `ipv6_required()` with its own unit test (`ipv6_required_ignores_unset_empty_and_zero`).
- `.github/workflows/ci.yml` (backend job, "Run tests" step): `CCS_REQUIRE_IPV6: ${{ runner.os != 'Windows' && '1' || '' }}` — set on the Linux and macOS legs; Windows gets an empty value, which the helper treats as unset (left out because Windows runner IPv6 loopback availability was not verified).
- Proxy fixture rule (CCS-034): this is a test-only change with no proxy behaviour change, so the PR needs the `no-fixture-needed` label applied by a maintainer.
- Commit: 956052f.

Verification (Linux container without IPv6):
- `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::hyper_client` — 16 passed, 0 failed (IPv6 tests skip).
- `CCS_REQUIRE_IPV6=1 cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::hyper_client::tests::connect_via_proxy_reaches_ipv6_literal_proxy` — FAILED as intended: `CCS_REQUIRE_IPV6 is set but IPv6 loopback bind failed: Address family not supported by protocol (os error 97)`.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` — 2852 passed, 0 failed, 3 ignored.
- YAML parses with `yaml.safe_load`; `~/go/bin/actionlint .github/workflows/ci.yml` — clean.
- Remaining (CI only): the real `[::1]` assertions run on the GitHub Linux/macOS backend legs.

Review:
- Checked all three acceptance criteria and the CI wiring; the env guard logic, the unit test and the Linux/macOS-only `CCS_REQUIRE_IPV6` value are correct, and `bind_ipv6_loopback()` is the only `[::1]` bind in the Rust tests. One fix: inserting `ipv6_required()` had left the "Bind a listener on `[::1]:0`" doc comment attached to the helper instead of `bind_ipv6_loopback()`; each function now has its own doc comment.
- Re-verified here: `--lib proxy::hyper_client` 16 passed; with `CCS_REQUIRE_IPV6=1` both `[::1]` tests panic with EAFNOSUPPORT; with `CCS_REQUIRE_IPV6=0` they skip; fmt, clippy `--all-targets -D warnings`, actionlint clean; full `--lib` 2852 passed, 0 failed, 3 ignored.
