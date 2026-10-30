# CCS-214: [deps] Refresh Cargo.lock and prune the cargo-deny advisory baseline

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P0`, `p2-medium`, `area:deps`

> **Ticket key:** `p0-deps-cargo-update-deny-baseline` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-018, CCS-001). Source tickets: [CCS-018](../P0-safety-net/CCS-018-p0-ci-cargo-deny.md), [CCS-001](../P0-safety-net/CCS-001-epic-p0.md).

## Problem
`src-tauri/deny.toml` `[advisories].ignore` (~lines 27-61) still carries all 30 baselined RUSTSEC IDs and the 2 yanked-crate entries from CCS-018. `src-tauri/Cargo.lock` still pins every affected version: aws-lc-sys 0.36.0, rustls 0.23.37, rustls-webpki 0.103.9, webpki-roots 1.0.6, h2 0.4.13, quick-xml 0.38.4, tar 0.4.44, zstd-sys 2.0.16, anyhow 1.0.102, tokio 1.50.0, clipboard-win 5.4.1, uds_windows 1.2.0. Most ignore reasons say "fix via cargo update", so the gate is weaker than it needs to be. The CCS-018 acceptance boxes are also still `[ ]` even though both were verified locally.

## Proposed change
- Run `cargo update --manifest-path src-tauri/Cargo.toml` (or targeted `-p aws-lc-sys -p aws-lc-rs -p rustls -p rustls-webpki -p webpki-roots -p webpki-root-certs -p h2 -p quick-xml -p tar -p zstd-sys -p anyhow -p tokio -p clipboard-win -p uds_windows`).
- Delete every ignore entry whose reason says "fix via cargo update", including the 2 yanked-crate entries.
- Keep the zip (RUSTSEC-2026-0367/0368/0369, see CCS-215) and zstd (0391/0398/0403) entries unless a compatible fix now resolves; if a fixed zstd major exists, note it for a follow-up (`src-tauri/Cargo.toml:51`, `proxy/content_encoding.rs`).
- Re-run the CCS-018 `lru = "=0.6.6"` scratch-pin check against the pruned `deny.toml`, then tick both CCS-018 acceptance boxes and leave only branch protection under Remaining.

## Acceptance criteria / verification
- [ ] `cargo deny --manifest-path src-tauri/Cargo.toml check` prints `advisories ok` with the pruned list (use `git-fetch-with-cli` per the CCS-018 notes if the proxy blocks the DB fetch).
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` is clean.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` passes, including `proxy_golden`, `proxy_e2e`, `config_writers` and the migration tests (rustls/tokio bumps can change behaviour).
- [ ] The `lru = "=0.6.6"` scratch pin still fails with RUSTSEC-2021-0130; CCS-018 boxes ticked.

## Dependencies
None.
