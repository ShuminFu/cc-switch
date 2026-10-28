# CCS-025: [ci] Use rust-toolchain.toml as the single toolchain source and fix the false MSRV

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci`

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

## Implementation notes

**Summary.** `rust-toolchain.toml` (channel 1.95 with rustfmt and clippy) is now the only place the toolchain is chosen. The `dtolnay/rust-toolchain@stable` steps in the `backend` job (`ci.yml`) and the release build (`release.yml`) are replaced with `rustup toolchain install`, which installs the pinned channel and its components, followed by `rustup show active-toolchain` so the setup log shows which single toolchain is active. In `release.yml` the `rustup target add` steps run after that step, so the targets go onto the pinned toolchain. This follows the plain-rustup approach the other CI jobs already used. `rust-version` is raised from 1.85.0 to 1.88.0; per `cargo metadata --locked`, the dependencies that need the most are time 0.3.47, time-core, time-macros, image 0.25.10 and home 0.5.12, all of which require 1.88. The README (EN/DE/JA/ZH) and CONTRIBUTING (EN/ZH) prerequisites now say Rust 1.88+. A new `msrv` job in `ci.yml` (ubuntu-22.04) runs `cargo +1.88 check --locked`, so a lockfile bump that silently raises the MSRV now fails CI.

**Files.** `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `src-tauri/Cargo.toml`, `README.md`, `README_DE.md`, `README_JA.md`, `README_ZH.md`, `CONTRIBUTING.md`, this ticket.

**Commits.** See `git log --grep CCS-025` (code and ticket notes in one commit).

**Verification.**
- `cd src-tauri && cargo +1.88 check --locked` (rustc 1.88.0, separate target dir): Finished, cc-switch v3.17.0 checked with rust-version = 1.88.0.
- `cd src-tauri && cargo check --locked` on the pinned 1.95: Finished. `cargo fmt --check`: clean. Cargo.lock unchanged.
- Max dependency `rust_version` from `cargo metadata --locked`: 1.88 (time, image, home); the next highest is 1.87 (zbus, wasip2), so 1.85 really was false.
- `python3 -c "import yaml,sys;yaml.safe_load(open(sys.argv[1]))"` on ci.yml and release.yml: both parse. actionlint was not available locally.
- Still to confirm on GitHub (needs a real runner): the CI setup log shows only the pinned 1.95 toolchain being installed or activated, including on the Windows and macOS legs, and the new `msrv` job passes.
