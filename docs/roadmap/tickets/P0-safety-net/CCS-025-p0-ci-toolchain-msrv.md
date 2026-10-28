# CCS-025: [ci] Use rust-toolchain.toml as the single toolchain source and fix the false MSRV

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci`

> **Ticket key:** `p0-ci-toolchain-msrv` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-rust-toolchain-mismatch. CI installs @stable, and rustup then switches to the pinned 1.95. The declared MSRV of 1.85 is false: time 0.3.47, image 0.25.10 and home 0.5.12 need 1.88.

## Evidence
- rust-toolchain.toml (1.95); ci.yml:68 and release.yml:41 dtolnay/rust-toolchain@stable; src-tauri/Cargo.toml:9 rust-version=1.85.0; README*.md:444, CONTRIBUTING.md:24/152

## Proposed change
- Use dtolnay/rust-toolchain@1.95 (SHA-pinned), or plain `rustup show`, and add targets afterwards.
- Raise rust-version to at least 1.88 and update the docs.
- Optionally add a `cargo +<msrv> check --locked` job.

## Acceptance criteria / verification
- [ ] `cd src-tauri && cargo +<new msrv> check --locked` passes
- [ ] The CI setup log shows one toolchain

## Dependencies
None.
