# CCS-214: [deps] Refresh Cargo.lock and prune the cargo-deny advisory baseline

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P0`, `p2-medium`, `area:deps`

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
- [x] `cargo deny --manifest-path src-tauri/Cargo.toml check` prints `advisories ok` with the pruned list (use `git-fetch-with-cli` per the CCS-018 notes if the proxy blocks the DB fetch).
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` is clean.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` passes, including `proxy_golden`, `proxy_e2e`, `config_writers` and the migration tests (rustls/tokio bumps can change behaviour).
- [x] The `lru = "=0.6.6"` scratch pin still fails with RUSTSEC-2021-0130; CCS-018 boxes ticked.

## Dependencies
None.

## Implementation notes

### Change
- `src-tauri/Cargo.lock`: targeted `cargo update -p aws-lc-sys -p aws-lc-rs -p rustls
  -p rustls-webpki -p webpki-roots -p webpki-root-certs -p h2 -p quick-xml -p tar
  -p zstd-sys -p anyhow -p tokio -p clipboard-win -p uds_windows`, then two extra steps:
  - anyhow 1.0.104, aws-lc-rs 1.19.0, aws-lc-sys 0.47.0, clipboard-win 5.4.3,
    h2 0.4.20, rustls 0.23.46, rustls-webpki 0.103.16, tar 0.4.46,
    uds_windows 1.2.1, webpki-roots / webpki-root-certs 1.0.11,
    zstd-sys 2.1.2.
  - tokio stayed on 1.50.0 under the plain `-p tokio` update (the resolver
    reported 1.54.1 as available but kept 1.50.0). Pinned it with
    `cargo update -p tokio --precise 1.54.1`; that also brings mio 1.2.4 and
    tokio-macros 2.7.2. tokio 1.54.1 declares rust-version 1.85, below our 1.88.
  - quick-xml 0.38 is pulled in only by `plist` (via tauri), and the fix is in
    quick-xml 0.41. `cargo update -p plist` (1.8.0 -> 1.10.0) moves it to
    quick-xml 0.41.0. No Cargo.toml change was needed.
- `src-tauri/deny.toml`: removed all 24 "fix via cargo update" advisory IDs and both
  yanked-crate entries. Six entries remain, each with a reason:
  - zip 2.x: RUSTSEC-2026-0367/0368/0369, tracked by CCS-215.
  - zstd 0.13 / zstd-safe 7: RUSTSEC-2026-0391/0398/0403.
  Also dropped the `OpenSSL` license allowance. After the aws-lc-sys bump no
  crate uses that license, and cargo-deny had started warning
  `license-not-encountered`.
- CCS-018: both acceptance boxes ticked; its Remaining now lists only branch protection.

### zstd follow-up
Fixed releases exist: zstd >= 0.14.3 and zstd-safe >= 8.2.0. Both are
breaking bumps for `zstd = "0.13"` (`src-tauri/Cargo.toml:51`, used in
`proxy/content_encoding.rs`). `zip 2.4.2` also depends on zstd 0.13, so the
zstd ignores can only go once the direct dependency is bumped and zip is
upgraded (CCS-215) to a release that uses zstd >= 0.14.3. This needs its own
follow-up ticket; it is not part of this change.

### Commits
- `1d2d7a5` chore(deps): refresh Cargo.lock and prune the cargo-deny baseline (CCS-214)
- (this ticket-notes commit)

### Verification
- `cargo deny --manifest-path src-tauri/Cargo.toml check` (cargo-deny 0.20.2): the
  result was `advisories ok, bans ok, licenses ok, sources ok`. The only
  warnings are the 78 duplicate-version warnings; none is `advisory-not-detected`.
  Before the lockfile refresh, the pruned list failed on RUSTSEC-2026-0194/0195
  (quick-xml) and RUSTSEC-2026-0386 (tokio). The plist and tokio steps above
  fixed both.
- lru scratch pin: on a copy of `src-tauri/` with the pruned deny.toml,
  `cargo deny check advisories` printed `advisories ok`. After adding
  `lru = "=0.6.6"` it exited 1 with `advisories FAILED`, reporting
  RUSTSEC-2021-0130 (use after free) and RUSTSEC-2026-0253.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 3097
  passed, 0 failed, 3 ignored (lib 2850). Every integration binary passed,
  including `proxy_golden`, `proxy_e2e`, `config_writers`, `migrations` and
  `takeover_crash`.
- No new regression test: this is a lockfile/config change. The
  cargo-deny gate is the regression check, and it is verified above in
  both directions.

