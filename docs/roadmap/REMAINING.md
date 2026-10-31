# What is left after the next-phase implementation

All 253 tickets in [tickets/](tickets/README.md) have been implemented and independently reviewed. 25 non-epic tickets and 5 of the 7 epics are marked **Partial**. Their code is finished and verified on Linux. What stops them from closing is something this Linux build container could not provide. This file groups those items by what they need, so whoever has that resource can close them. Each ticket's own "Remaining after triage" and audit notes have the details.

Verification at the final merge (`673be73`): `cargo test --no-fail-fast` passed 3135 tests across 25 binaries. `pnpm test:unit` passed 1797 tests in 180 files. `cargo clippy --all-targets -D warnings`, `pnpm typecheck`, `pnpm lint`, `pnpm format:check` and the IPC orphan check all passed.

## Needs a real GitHub Actions run

This fork has no Actions history, and CI only runs for pushes or PRs to `main`. One PR run from this branch to `main` covers most of the list.

- CCS-201 / CCS-007: the `tauri-build-smoke` job and the Linux AppImage smoke pass, and a failing smoke blocks the upload.
- CCS-019: coverage artifacts with per-directory summaries appear on a PR run.
- CCS-026: the CI log prints Node v22.12.0.
- CCS-027: the GITHUB_TOKEN permissions show only Contents: read and Metadata: read.
- CCS-043: run the stale workflow with `workflow_dispatch` and `debug-only: true`.
- CCS-014 / CCS-213: the proxy_e2e perf artifact uploads from the backend matrix.
- CCS-034: `proxy-fixture.yml` passes or fails correctly on a real PR.
- CCS-006: the gated `coverage-rust` job passes (floors of 75% for proxy/ and database/; locally they measure 92% and 86%).

## Needs macOS or Windows runners or machines

Cross-compiling from Linux fails in `aws-lc-sys`. Wine runs give interim evidence only.

- CCS-014 / CCS-001 / CCS-004: proxy_e2e and the golden suite pass on macOS and Windows.
- CCS-152 / CCS-005: `takeover_crash` passes on all 3 OSes (Linux: 10/10).
- CCS-036: the cfg-gated macOS and Windows terminal branches.
- CCS-079 / CCS-083 / CCS-002: Windows `rename_replace` and the owner-only DACL tests on `windows-latest`.
- CCS-100: clippy on macOS, plus a check that keychain prompts stay responsive on a Mac.
- CCS-179: a native macOS run of the terminal tests.

## Needs a host with IPv6 loopback

- CCS-061: the `[::1]` CONNECT tests have to actually run. Since CCS-227, CI with `CCS_REQUIRE_IPV6=1` fails instead of silently skipping them.

## Needs a tag or release run on a fork, with signing secrets

- CCS-023: a tag on a commit with red CI does not release.
- CCS-029: `.sig` files on all 3 OSes. CCS-220 covered the Linux leg locally.
- CCS-031: a re-pushed tag waits for the in-progress publish instead of cancelling it.
- CCS-032: promoting a prerelease, plus the `releases/latest` redirect check.
- CCS-033: SHA256SUMS in the release assets, and `gh attestation verify` passes.

## Needs a maintainer to change GitHub repository settings

- CCS-018 / CCS-034 / CCS-201: make cargo-deny, pnpm audit, Proxy fixture and the e2e smoke required checks.
- CCS-034: create the `no-fixture-needed` and `fixtures` labels.
- CCS-023: add required reviewers to the `release` environment.
- CCS-029: confirm that the `TAURI_SIGNING_PRIVATE_KEY` secret uses the canonical `.key` format.
- GitHub Issues are disabled on this fork, which is why the tickets live in the repo. Turn Issues on if you want them mirrored there.

## Needs a person with a real desktop or real accounts

- CCS-036 / CCS-184: launching and resuming a session in real terminals (GNOME Terminal, Konsole, Windows Terminal, cmd, PowerShell). CCS-223, CCS-248 and CCS-249 are the headless and Wine stand-ins.
- CCS-172: the keychain on macOS, Credential Manager on Windows, and KWallet on a Linux desktop. CCS-240 covers the Secret Service API.

## Needs time to pass, or a product decision

- CCS-178 / CCS-006: delete the legacy transform and streaming files (about 18.4k lines) and the `codecEngine` flag once one release has shipped with `codecEngine=ir` (the default since CCS-246) with no regressions.
- CCS-178 / CCS-006: the plan's target of at least 8k net lines removed. Deleting the legacy files nets about -2.8k, so the target needs revising.
- CCS-006: the proxy-fix-commits-per-release KPI needs v4.0 to ship.
- CCS-173: whether the sync key becomes mandatory, and whether signatures are bound to the remote location or profile. Both are protocol and product decisions.
- CCS-018: the zstd advisories (RUSTSEC-2026-0391/0398/0403) wait on an upstream fixed release.
