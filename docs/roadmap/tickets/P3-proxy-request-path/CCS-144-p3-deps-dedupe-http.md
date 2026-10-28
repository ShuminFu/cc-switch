# CCS-144: [deps] Dedupe tower, tower-http, webpki-roots and reqwest versions

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P3`, `p3-low`, `area:deps`

> **Ticket key:** `p3-deps-dedupe-http` · **Phase:** P3 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p0-unused-tower-http`, `p0-proxy-harness-e2e`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-duplicate-http-stacks, partially real. The app's direct deps (tower 0.4, webpki-roots 0.26, brotli 7, dirs 5, zip 2.x) plus reqwest 0.12 next to tauri-plugin-updater's 0.13 compile 73 to 79 duplicate crates.

## Evidence
- src-tauri/Cargo.toml:39-69; `cargo tree -d`

## Proposed change
Move to tower 0.5 and webpki-roots 1.0. Evaluate reqwest 0.13 (the rustls feature names changed). brotli 8 and dirs 6 are optional.

## Acceptance criteria / verification
- [ ] `cargo tree -d -e normal --depth 0 | sort -u | wc -l` drops; `cargo tree -i reqwest` shows one version if migrated
- [ ] proxy_e2e and the golden tests pass; release binary size and clean build time compared in the PR

## Dependencies
p0-unused-tower-http, p0-proxy-harness-e2e
