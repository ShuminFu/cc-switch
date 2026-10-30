# CCS-215: [deps] Upgrade the direct zip dependency to a release that fixes RUSTSEC-2026-0367/0368/0369

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P0`, `p2-medium`, `area:deps`

> **Ticket key:** `p0-deps-zip-upgrade` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p0-deps-cargo-update-deny-baseline`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-018, CCS-001). Source tickets: [CCS-018](../P0-safety-net/CCS-018-p0-ci-cargo-deny.md), [CCS-001](../P0-safety-net/CCS-001-epic-p0.md).

## Problem
`src-tauri/Cargo.toml:73` has `zip = "2.2"` (resolves to 2.4.2), which is affected by RUSTSEC-2026-0367/0368/0369; the three IDs sit in the `deny.toml` ignore list. A second copy, zip 4.6.1, comes in through tauri-plugin-updater. Fixing it needs a semver-major bump and call-site changes, so it does not fit in the plain `cargo update` of CCS-214.

## Proposed change
- Bump `zip` to the first release that fixes the three advisories (check with `cargo deny` / the RustSec DB).
- Adapt the call sites: `src-tauri/src/services/skill.rs`, `services/webdav_sync/archive.rs`, `proxy/capture/export.rs`, `proxy/capture/tests.rs`, `commands/skill.rs`.
- If a tauri-plugin-updater update moves its copy to a fixed zip, take it. Otherwise keep the ignore entries but reword them to name only the transitive updater copy.
- `proxy/capture/*` is under `src-tauri/src/proxy/`, so the PR needs a fixture change or the `no-fixture-needed` label (CCS-034).

## Acceptance criteria / verification
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` passes, including skill import/export, the WebDAV archive and capture export tests.
- [ ] `cargo deny --manifest-path src-tauri/Cargo.toml check advisories` passes with the three zip ignores removed, or narrowed to the updater copy.
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` is clean.

## Dependencies
CCS-214 (`p0-deps-cargo-update-deny-baseline`).
