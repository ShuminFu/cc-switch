# What is left after the next-phase implementation

> **All 253 tickets are closed (Status: Done), but not everything was verified.** Two owner decisions closed the last tickets without their remaining checks:
>
> - **2026-10-31:** every check that needs a GitHub Actions run was skipped. That closed CCS-004, CCS-014, CCS-019, CCS-026, CCS-027, CCS-031, CCS-032, CCS-033, CCS-043, CCS-079, CCS-083, CCS-152, CCS-179 and CCS-213.
> - **2026-11-01:** every other remaining item was skipped too: repository settings, manual checks on real machines, the IPv6-host test run, the release soak with the legacy codec deletion and the v4.0 KPI, and the open product decisions. That closed the last 16 Partial tickets: CCS-001, CCS-002, CCS-006, CCS-007, CCS-018, CCS-023, CCS-029, CCS-034, CCS-036, CCS-061, CCS-100, CCS-172, CCS-173, CCS-178, CCS-184 and CCS-201.
>
> The items below were **skipped, not verified and not performed**. Closing a ticket does not mean these checks passed. Each affected ticket has an "Owner decision" section that lists them. Epic criteria ticked by the decisions carry "(skipped: needs GitHub Actions, owner decision)" or "(skipped: owner decision 2026-11-01, not verified)".

Status: 253 Done, 0 Partial, 0 Open ([ticket index](tickets/README.md)).

What was verified: all code is finished, reviewed and tested on Linux. At the final merge (`673be73`), `cargo test --no-fail-fast` passed 3135 tests across 25 binaries, and `pnpm test:unit` passed 1797 tests in 180 files. `cargo clippy --all-targets -D warnings`, `pnpm typecheck`, `pnpm lint`, `pnpm format:check` and the IPC orphan check all passed. Under Wine 9.0 the Windows cross-build passed 3092 tests with 1 failure, Wine's missing protected-DACL support (CCS-083), which is interim evidence only.

## Follow-up checklist for maintainers

Nothing below has been done. Tick an item when you have done or verified it, and add the result to the ticket named.

### 1. Needs a GitHub Actions run

This fork has no Actions history. CI runs only for pushes and PRs to `main`, so push to `main` or open a PR. Release items need a tag run on a fork with the signing secrets.

PR or push CI:
- [ ] CCS-201 / CCS-007: the `tauri-build-smoke` job passes on ubuntu-22.04.
- [ ] CCS-019: coverage artifacts with per-directory summaries for proxy/ and database/ appear on a PR run.
- [ ] CCS-006: the gated `coverage-rust` job passes with floors of 75% for proxy/ and database/. Local measurements: 92.27% and 85.70%.
- [ ] CCS-026: the CI log prints Node v22.12.0.
- [ ] CCS-027: the "Set up job" log shows GITHUB_TOKEN with only Contents: read and Metadata: read.
- [ ] CCS-043: run the stale workflow with `workflow_dispatch` and `debug-only: true`.
- [ ] CCS-034: `proxy-fixture.yml` passes or fails correctly on a real PR.
- [ ] CCS-018: a real run of `audit.yml` (cargo-deny and pnpm audit), including the weekly schedule.
- [ ] CCS-014 / CCS-213: the `proxy-e2e-perf-<os>` artifact uploads from the backend matrix on all 3 OSes.

macOS and Windows legs of the CI matrix:
- [ ] CCS-014 / CCS-001 / CCS-004: proxy_e2e and the golden suite pass on macOS and Windows, including the 3x allocation gate and the 1.1 s half-open wait.
- [ ] CCS-152: `takeover_crash` passes on all 3 OSes. Linux and Wine each passed 10/10.
- [ ] CCS-036: the cfg-gated macOS and Windows terminal branches compile and run.
- [ ] CCS-079 / CCS-083 / CCS-002: rename-without-delete and the owner-only DACL test `config::tests::atomic_write_secret_sets_owner_only_acl` pass on `windows-latest`.
- [ ] CCS-100 / CCS-002: `cargo clippy` passes on the macOS runner.
- [ ] CCS-179 / CCS-006: the terminal tests run natively on macOS (`osascript`/`open` launchers).
- [ ] CCS-061 / CCS-002: the Linux or macOS backend leg passes with `CCS_REQUIRE_IPV6=1` (see section 4 for the local alternative).

Tag or release runs on a fork, with signing secrets:
- [ ] CCS-201 / CCS-007: the AppImage smoke passes on the ubuntu-22.04 and ubuntu-22.04-arm release legs, and a failing smoke blocks the upload.
- [ ] CCS-023: a `v*` tag on a commit with red CI skips `release`, `publish-release` and `assemble-latest-json`.
- [ ] CCS-029: `.sig` files are produced on macOS and Windows. CCS-220 covered the Linux leg locally.
- [ ] CCS-031: a re-pushed tag waits for the in-progress publish instead of cancelling it.
- [ ] CCS-032: promote a prerelease, then check the `releases/latest` redirect with `curl -sIL`.
- [ ] CCS-033: SHA256SUMS is in the release assets and `gh attestation verify` passes.

### 2. Needs a maintainer to change GitHub repository settings

- [ ] CCS-018 / CCS-001: make `cargo-deny` and `pnpm audit` required status checks on `main` (Settings > Branches or Rulesets). A check can be selected only after it has run once.
- [ ] CCS-034 / CCS-001: make `Proxy fixture required` a required status check on `main`.
- [ ] CCS-034 / CCS-001: decide what the `no-fixture-needed` and `fixtures` labels mean, then create them. No decision has been taken.
- [ ] CCS-006: label the CCS-244 and CCS-246 PRs with a CCS-034 fixture label once the labels exist.
- [ ] CCS-201 / CCS-007: make the `tauri-build-smoke` E2E job a required status check on `main`.
- [ ] CCS-023 / CCS-001: add required reviewers to the `release` environment (Settings > Environments > `release`).
- [ ] CCS-029 / CCS-001: someone with secrets access confirms that `TAURI_SIGNING_PRIVATE_KEY` holds the canonical single-line base64 `.key` contents written by `tauri signer generate`, and re-saves it if not. The `Validate Tauri signing key` step fails a release run on any other form.
- [ ] Optional: GitHub Issues are disabled on this fork, which is why the tickets live in the repo. Turn Issues on if you want them mirrored there.

### 3. Needs a person on a real machine

- [ ] CCS-036 / CCS-001: on a real GNOME Terminal or Konsole with the real `claude` CLI, check that the config file is `-rw-------`, that it is removed after `claude` exits, and that quick A-then-B launches each get their own path. CCS-223 is only a headless xterm stand-in.
- [ ] CCS-184 / CCS-006: resume a session on a real Windows desktop (cmd /K, PowerShell -NoExit, Windows Terminal, the real npm `claude.cmd` shim) and in a real GNOME Terminal and Konsole. CCS-248 and CCS-249 are only headless and Wine stand-ins.
- [ ] CCS-100 / CCS-002: on a real Mac with a real Keychain entry, check that other commands stay responsive while a Keychain prompt is open, and that the quota falls back to the credentials file after 20 s if the prompt is ignored.
- [ ] CCS-172, macOS: Keychain Access entries, prompts and the downgrade path. The checklist is in `docs/dev/SECRET_STORAGE.md`.
- [ ] CCS-172, Windows: Credential Manager entries, the owner-only ACL on the fallback `secrets.json`, and the PowerShell downgrade.
- [ ] CCS-172, Linux desktop: KWallet with real unlock prompts. CCS-240 covers Secret Service only at the API level.

### 4. Needs a host with IPv6 loopback

- [ ] CCS-061 / CCS-002: run `CCS_REQUIRE_IPV6=1 cargo test --manifest-path src-tauri/Cargo.toml proxy::hyper_client` on any machine with IPv6 loopback, and confirm that `connect_via_proxy_reaches_ipv6_literal_proxy` and `send_raw_request_reaches_ipv6_literal_upstream` pass. With the variable set, they fail instead of skipping when IPv6 is missing. The build container returned EAFNOSUPPORT, so these tests have never run their assertions.

### 5. Needs a release soak, the legacy deletion and the v4.0 release

The legacy codec files have **not** been deleted, and the `codecEngine = legacy` fallback is still in place. `codecEngine = ir` has been the code default since CCS-246, but no release has shipped with it.

- [ ] CCS-178 / CCS-006: ship a release with `codecEngine = ir` as the default.
- [ ] CCS-178 / CCS-006: after one release with no regressions, delete the 10 legacy `providers/transform*.rs` and `streaming*.rs` files (about 18.4k lines), the `CodecEngine` flag and the `codecEngine` setting, the legacy arms in `codec/bridge.rs`, the test-only legacy imports and the codec_parity tests.
- [ ] CCS-178 / CCS-006: after the deletion, empty the `scripts/check-usage-token-names.mjs` allowlist so that `cached_tokens` and `cache_creation_input_tokens` appear only in the codec and usage modules.
- [ ] CCS-006: ship v4.0 with release tags, record a P0 baseline count of proxy fix commits, and measure the proxy-fix-commits-per-release KPI.
- [ ] CCS-018: adopt an upstream fix for the zstd advisories (RUSTSEC-2026-0391/0398/0403), then drop them from the `src-tauri/deny.toml` ignore list.

### 6. Open product and maintainer decisions

No decision was taken on these. The current behaviour stays as implemented.

- [ ] CCS-178 / CCS-006: the plan's target of at least 8k net lines removed. Deleting the legacy files nets only about -2.8k. Either revise the target, or trim the codec tests that duplicate legacy tests when the parity tests are removed. The target is unchanged and not met.
- [ ] CCS-178: decide whether `stream/taps.rs`, `services/session_usage.rs` and `providers/testkit.rs` count as usage or test modules for the token-name criterion. CCS-244 has since moved the field reads out of them.
- [ ] CCS-173: decide whether the sync key becomes mandatory once the sync-key UI has shipped in a release. For now the key is optional. Without a key, uploads are unsigned and unsigned snapshots can be imported. With a key, an unsigned snapshot needs the one-time migration confirmation from CCS-238.
- [ ] CCS-173: decide whether the HMAC signature is bound to the remote location or profile. Doing so needs a manifest version bump. For now it is not bound.
- [ ] CCS-034: decide what the fixture labels mean (see section 2).
