# What is left after the next-phase implementation

> **Owner decision (2026-10-31): every check that needs a GitHub Actions run is skipped.** The repository owner decided not to wait for GitHub Actions runs (PR CI, the macOS/Windows matrix legs, tag and release runs on a fork). Those checks are **skipped, not verified**: no green Actions run has been observed for any of them. Each affected ticket has an "Owner decision: GitHub Actions checks skipped" section, and epic criteria that were only waiting on Actions are ticked with "(skipped: needs GitHub Actions, owner decision)". Tickets whose only blockers were Actions runs are now **Done**: CCS-004, CCS-014, CCS-019, CCS-026, CCS-027, CCS-031, CCS-032, CCS-033, CCS-043, CCS-079, CCS-083, CCS-152, CCS-179, CCS-213. Status now: 237 Done, 16 Partial (12 non-epic tickets plus epics CCS-001, CCS-002, CCS-006, CCS-007).

All 253 tickets in [tickets/](tickets/README.md) have been implemented and independently reviewed. 25 non-epic tickets and 5 of the 7 epics were marked **Partial** before the owner decision above; 12 non-epic tickets and 4 epics are still **Partial**. Their code is finished and verified on Linux. What stops them from closing is something this Linux build container could not provide. This file groups those items by what they need, so whoever has that resource can close them. Each ticket's own "Remaining after triage" and audit notes have the details.

Verification at the final merge (`673be73`): `cargo test --no-fail-fast` passed 3135 tests across 25 binaries. `pnpm test:unit` passed 1797 tests in 180 files. `cargo clippy --all-targets -D warnings`, `pnpm typecheck`, `pnpm lint`, `pnpm format:check` and the IPC orphan check all passed.

## Still remaining (not GitHub Actions)

These are the only items still keeping tickets Partial. None of them is an Actions run.

### Needs a maintainer to change GitHub repository settings

- CCS-201 / CCS-007: make the `tauri-build-smoke` E2E job a required status check. Once this is done, CCS-201 and CCS-007 can be set to Done.
- CCS-018 / CCS-001: make `cargo-deny` and `pnpm audit` required checks on `main`.
- CCS-034 / CCS-001: make `Proxy fixture required` a required check on `main`, and create the `no-fixture-needed` and `fixtures` labels (needs a human decision on the labels).
- CCS-006: label the CCS-244 and CCS-246 PRs with a CCS-034 fixture label (`fixtures` or `no-fixture-needed`). This does not block an exit criterion.
- CCS-023 / CCS-001: add required reviewers to the `release` environment.
- CCS-029 / CCS-001: someone with secrets access confirms that `TAURI_SIGNING_PRIVATE_KEY` holds the canonical single-line base64 `.key` contents, and re-saves it if not.
- GitHub Issues are disabled on this fork, which is why the tickets live in the repo. Turn Issues on if you want them mirrored there (not a ticket blocker).

### Needs a host with IPv6 loopback

- CCS-061: the `[::1]` CONNECT tests (`connect_via_proxy_reaches_ipv6_literal_proxy`, `send_raw_request_reaches_ipv6_literal_upstream`) have to actually run and pass. One `cargo test` run on any dev machine with IPv6 loopback is enough; it does not need GitHub Actions. This container returns EAFNOSUPPORT, so the tests skip early.

### Needs a person with a real desktop, a real Mac or Windows machine, or real accounts

- CCS-036 / CCS-001: on a real GNOME Terminal or Konsole with the real `claude` CLI, check that the config file is `-rw-------`, is removed after `claude` exits, and that quick A-then-B launches each get their own path. CCS-223 is the headless xterm stand-in.
- CCS-184 / CCS-006: resume a session on a real Windows desktop (cmd /K, PowerShell -NoExit, Windows Terminal, the real npm `claude.cmd` shim) and in real GNOME Terminal/Konsole. CCS-248 and CCS-249 are the headless and Wine stand-ins.
- CCS-100 / CCS-002: on a real Mac with a real Keychain entry, check that other commands stay responsive while a Keychain prompt is open, and that the quota falls back to the credentials file after 20 s if the prompt is ignored.
- CCS-172: the keychain on macOS (Keychain Access entries, prompts, downgrade), Credential Manager and the owner-only ACL on the fallback `secrets.json` on Windows, and KWallet with real unlock prompts on a Linux desktop. CCS-240 covers the Secret Service API. Checklist in `docs/dev/SECRET_STORAGE.md`.

### Needs time to pass, or a product or maintainer decision

- CCS-178 / CCS-006: ship a release with `codecEngine=ir` as the default (the default since CCS-246), then, after one release with no regressions, delete the legacy transform and streaming files (about 18.4k lines), the `codecEngine` flag, the legacy bridge arms and the codec_parity tests.
- CCS-178 / CCS-006: the plan's target of at least 8k net lines removed. Deleting the legacy files nets about -2.8k, so a maintainer has to revise the target or trim duplicated codec tests.
- CCS-178 / CCS-006: `cached_tokens`/`cache_creation_input_tokens` only in codec and usage modules. This follows from the legacy deletion, which empties the `check-usage-token-names.mjs` allowlist.
- CCS-006: the proxy-fix-commits-per-release KPI needs v4.0 to ship with release tags and a recorded P0 baseline count.
- CCS-173: whether the sync key becomes mandatory (after the sync-key UI ships), and whether signatures are bound to the remote location or profile (needs a manifest version bump). Both are protocol and product decisions.
- CCS-018: the zstd advisories (RUSTSEC-2026-0391/0398/0403) wait on an upstream fixed release.

## Skipped by owner decision (needs GitHub Actions)

Skipped on 2026-10-31 by the repository owner's decision. These are **not verified**; they would need a real GitHub Actions run (this fork has no Actions history, and CI only runs for pushes or PRs to `main`).

### PR / push CI runs

- CCS-201 / CCS-007: the `tauri-build-smoke` job passes on ubuntu-22.04.
- CCS-019: coverage artifacts with per-directory summaries appear on a PR run.
- CCS-026: the CI log prints Node v22.12.0.
- CCS-027: the GITHUB_TOKEN permissions show only Contents: read and Metadata: read.
- CCS-043: run the stale workflow with `workflow_dispatch` and `debug-only: true`.
- CCS-014 / CCS-213: the proxy_e2e perf artifact uploads from the backend matrix on all 3 OSes.
- CCS-034: `proxy-fixture.yml` passes or fails correctly on a real PR.
- CCS-018: a real run of `audit.yml` (cargo-deny and pnpm audit, including the weekly schedule).
- CCS-006: the gated `coverage-rust` job passes (floors of 75% for proxy/ and database/; locally they measure 92% and 86%).

### macOS and Windows legs of the CI matrix


With mingw installed the whole crate now cross-compiles to `x86_64-pc-windows-gnu`. Under Wine 9.0 (`LANG=LC_ALL=C.UTF-8`, at `bdd666e`) the full suite gives 3092 passed, 1 failed, 3 ignored. Linux gives 3136 passed, 0 failed. Both clippy runs (Linux and `--target x86_64-pc-windows-gnu`, `--all-targets -D warnings`) are clean.

Wine found 13 failures at `a04dafe`. Twelve were fixed:
- 9 came from a test-isolation bug: the injected `Paths` ignored `%LOCALAPPDATA%`, so tests wrote to the real user's Hermes and Claude Desktop directories. Fixed in `4385f6f`.
- 2 were POSIX-only assumptions in tests: the Tauri local origin, and `~/.hermes`.
- 1 was Wine's stubbed `CreateSymbolicLinkW`. Skill sync now falls back to copying, in `d83fd0b`.
- 1 needed a UTF-8 locale for non-ASCII filenames under Wine.

The remaining failure is Wine not keeping a protected DACL (CCS-083). That is a Wine gap, not a product bug. Wine is interim evidence only.

- CCS-014 / CCS-001 / CCS-004: proxy_e2e and the golden suite pass on macOS and Windows.
- CCS-152: `takeover_crash` passes on all 3 OSes. Linux and Wine both pass 10/10.
- CCS-036: the cfg-gated macOS and Windows terminal branches compile and run.
- CCS-079 / CCS-083 / CCS-002: rename-without-delete and the owner-only DACL test on `windows-latest`. Wine cannot check the DACL, because it does not keep DACLs. `rename_replace` already passes under Wine.
- CCS-100 / CCS-002: `cargo clippy` on the macOS runner.
- CCS-179 / CCS-006: a native macOS run of the terminal tests.
- CCS-061 / CCS-002: the CI run with `CCS_REQUIRE_IPV6=1` (any dev machine with IPv6 loopback can stand in, see above).

### Tag or release runs on a fork, with signing secrets

- CCS-201 / CCS-007: the AppImage smoke on the ubuntu-22.04 and ubuntu-22.04-arm release legs, and a failing smoke blocks the upload.
- CCS-023: a tag on a commit with red CI does not release.
- CCS-029: `.sig` files on all 3 OSes. CCS-220 covered the Linux leg locally.
- CCS-031: a re-pushed tag waits for the in-progress publish instead of cancelling it.
- CCS-032: promoting a prerelease, plus the `releases/latest` redirect check.
- CCS-033: SHA256SUMS in the release assets, and `gh attestation verify` passes.
