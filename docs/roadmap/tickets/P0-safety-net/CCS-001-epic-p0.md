# CCS-001: [epic] P0 (v3.18): Safety net, real CI gates and zero-risk hotfixes

**Status:** Partial · **Kind:** epic · **Labels:** `epic`, `phase:P0`, `p0-critical`, `area:tests`

> **Ticket key:** `epic-p0` · **Phase:** P0 · **Priority:** p0-critical · **Effort:** XL
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Pin current behaviour in the areas that regress most (protocol bridges, cost accounting, migrations, live-config writers, proxy lifecycle) and make CI enforce what CONTRIBUTING.md claims. Ship a handful of hotfixes that need no harness.

## Exit criteria
- [x] At least 60 golden bridge fixtures across all 5 bridges (stream and non-stream), passing under every-offset, random, UTF-8 and CRLF chunk splitting; the ~1046 existing inline proxy tests still pass — *audit: 182 `meta.toml` fixtures under `src-tauri/tests/fixtures/proxy/` (anthropic__openai_chat 39, anthropic__gemini 32, anthropic__responses 34, responses__chat 31, responses__anthropic 29, plus gemini__anthropic 17); split plans `byte-offset`/`utf8`/`crlf`/`random#N` in `proxy/providers/testkit.rs`; `cargo test --test proxy_golden` 18/18 passed; `cargo test --lib proxy::` 1524 passed, 0 failed.*
- [x] Cost golden table covers 8 apps x 3 semantics; the only failures are the known grokbuild backfill rows, marked #[ignore] — *audit: `proxy/usage/cost_golden.rs` iterates `AppType::all()` (asserted == 8) x LEGACY/TOTAL/FRESH; the grokbuild divergence was since fixed by CCS-092 (Done), so the `#[ignore]` is gone and `cargo test --lib cost_golden` gives 7 passed, 0 failed, 0 ignored (stronger than the criterion).*
- [x] Migration harness passes from at least 10 start versions plus the legacy config.json path (migrate_from_json) — *audit: 20 DB fixtures (`tests/fixtures/db/v1..v21.sql`, no v14) plus `legacy/config-v3.6-per-app-mcp.json` and `config-v3.7-unified-mcp.json` through `migrate_from_json`; `cargo test --test migrations` 14/14 passed.*
- [x] Config-writer snapshots exist for all 11 live writers plus the Copilot/Codex OAuth token stores and the Codex state DB; untouched bytes stay identical — *audit: `tests/fixtures/live/` has the 11 writers (claude_settings, claude_mcp, claude_plugin, claude_desktop, codex, gemini, grok, hermes, openclaw, openclaw_mcp, opencode) plus copilot_auth, codex_oauth, codex_state_db (and codex_preserve_auth), each with lf/crlf/bom snapshots and byte-identity asserts; `cargo test --test config_writers` 19/19 passed.*
- [ ] test_mutex is removed, integration tests run in parallel, and proxy_e2e has at least 8 scenarios, green on 3 OSes — *audit: `grep -rn test_mutex src-tauri` finds nothing; TestEnv injects per-thread `Paths` (`test_env_isolation` 2/2 passed); `proxy_e2e` has 37 scenarios, 37/37 passed on Linux. Not ticked: "green on 3 OSes" is unverified. The CI `backend` matrix (ubuntu-22.04, windows-latest, macos-latest) runs it, but no Actions run is visible on the fork (`gh run list` is empty).*
- [x] CI enforces clippy --all-targets, ESLint (baseline), Prettier on tests/, cargo-deny and i18n parity; a tag with no green CI cannot publish — *audit: ci.yml runs `cargo clippy --all-targets -- -D warnings`, `pnpm lint` (eslint, `--max-warnings 0`, baseline ratchet), `pnpm format:check` (`{src,tests}/**`) and `pnpm test:unit` (includes `tests/i18n/localeParity` + `usedKeys`); audit.yml runs cargo-deny-action; release.yml chains `ci (uses ci.yml) + check-versions -> release -> publish-release -> assemble-latest-json` with no `always()` on the chain. Making these required checks on `main` is a GitHub setting (CCS-018), and a live fork-tag test is still open (CCS-023).*
- [x] Release pipeline: SHA-pinned actions, secrets passed via env:, missing updater .sig is fatal — *audit: every `uses:` in `.github/workflows/*.yml` is pinned to a 40-hex SHA (grep finds no exceptions apart from local `./` workflows); every `secrets.*` in release.yml is a `NAME: ${{ secrets.X }}` env mapping (guarded by `tests/ci/releaseSecrets.test.ts`); macOS/Windows/Linux packaging steps `exit 1` on a missing or empty `.sig` (CCS-030). A signed fork-tag run remains (CCS-029).*
- [x] No log line or webview event contains an unredacted ccswitch:// URL — *audit: all deeplink entry points (single-instance args, `on_open_url`, macOS `RunEvent::Opened`) go through `app_shell/run_events.rs::route_deeplinks`, which logs only `deeplink::redact_url_for_log(..)`; `deeplink-error` carries `deeplink_error_payload` (redacted URL, test `error_payload_does_not_contain_secret`); `deeplink-import` carries the parsed request, not the URL; the frontend logs only `event.payload.error`.*

## Child tickets
p0-golden-testkit-chat-gemini, p0-golden-responses-fixtures, p0-cost-golden-table, p0-migration-harness, p0-config-writer-snapshots, p0-testenv-paths, p0-proxy-harness-e2e, p0-untested-module-tests, p0-ci-clippy-all-targets, p0-ci-eslint-prettier, p0-ci-cargo-deny, p0-ci-coverage-report, p0-ci-i18n-parity, p0-i18n-missing-keys, p0-i18n-zhtw-keys, p0-ci-release-gated, p0-ci-build-smoke, p0-ci-toolchain-msrv, p0-ci-node-version, p0-ci-permissions, p0-ci-pin-actions, p0-ci-release-secrets-env, p0-ci-updater-sig-fatal, p0-ci-release-concurrency, p0-ci-prerelease-promotion, p0-ci-release-hygiene, p0-dev-acceptance-process, p0-hotfix-deeplink-redaction, p0-hotfix-terminal-tempfiles, p0-hotfix-sync-skip-session-log, p0-hotfix-session-sync-blocking, p0-hotfix-providerform-parse, p0-failure-threshold-const, p0-unused-tower-http, p0-icon-scripts-cjs, p0-ci-stale-workflow, p0-flatpak-manifest, p0-docs-changelog-unreleased, p0-docs-readme-release-sync, p0-docs-package-description, p0-docs-root-clutter

## Epic closure audit

Audited 2026-10-30 against branch `claude/funny-carson-9c7j3e`. Commands run (read-only, Linux x86_64): `cargo test --test proxy_golden` (18/18), `--test migrations` (14/14), `--test config_writers` (19/19), `--test proxy_e2e` (37/37), `--test test_env_isolation` (2/2), `--lib proxy::` (1524 passed, 0 failed), `--lib cost_golden` (7 passed, 0 ignored), and greps over `.github/workflows/`, `src-tauri/src/app_shell/run_events.rs` and `src-tauri/src/deeplink/`.

### Child tickets (41: 27 Done, 14 Partial)

| Ticket | Key | Status | What remains (Partial only) |
| --- | --- | --- | --- |
| [CCS-008](CCS-008-p0-golden-testkit-chat-gemini.md) | `p0-golden-testkit-chat-gemini` | Done |  |
| [CCS-009](CCS-009-p0-golden-responses-fixtures.md) | `p0-golden-responses-fixtures` | Done |  |
| [CCS-010](CCS-010-p0-cost-golden-table.md) | `p0-cost-golden-table` | Done |  |
| [CCS-011](CCS-011-p0-migration-harness.md) | `p0-migration-harness` | Done |  |
| [CCS-012](CCS-012-p0-config-writer-snapshots.md) | `p0-config-writer-snapshots` | Done |  |
| [CCS-013](CCS-013-p0-testenv-paths.md) | `p0-testenv-paths` | Done |  |
| [CCS-014](CCS-014-p0-proxy-harness-e2e.md) | `p0-proxy-harness-e2e` | Partial | macOS/Windows results via the CI backend matrix; uploading the perf JSON as a CI artifact |
| [CCS-015](CCS-015-p0-untested-module-tests.md) | `p0-untested-module-tests` | Done |  |
| [CCS-016](CCS-016-p0-ci-clippy-all-targets.md) | `p0-ci-clippy-all-targets` | Done |  |
| [CCS-017](CCS-017-p0-ci-eslint-prettier.md) | `p0-ci-eslint-prettier` | Done |  |
| [CCS-018](CCS-018-p0-ci-cargo-deny.md) | `p0-ci-cargo-deny` | Partial | mark the new checks as required in branch protection (GitHub setting); `cargo update` to clear the baselined RUSTSEC ignores |
| [CCS-019](CCS-019-p0-ci-coverage-report.md) | `p0-ci-coverage-report` | Partial | confirm on a real PR run that both coverage artifacts upload |
| [CCS-020](CCS-020-p0-ci-i18n-parity.md) | `p0-ci-i18n-parity` | Done |  |
| [CCS-021](CCS-021-p0-i18n-missing-keys.md) | `p0-i18n-missing-keys` | Done |  |
| [CCS-022](CCS-022-p0-i18n-zhtw-keys.md) | `p0-i18n-zhtw-keys` | Done |  |
| [CCS-023](CCS-023-p0-ci-release-gated.md) | `p0-ci-release-gated` | Partial | fork test: a `v*` tag on a red commit must skip release; set required reviewers on the `release` environment (GitHub setting) |
| [CCS-024](CCS-024-p0-ci-build-smoke.md) | `p0-ci-build-smoke` | Done |  |
| [CCS-025](CCS-025-p0-ci-toolchain-msrv.md) | `p0-ci-toolchain-msrv` | Done |  |
| [CCS-026](CCS-026-p0-ci-node-version.md) | `p0-ci-node-version` | Partial | confirm that the Actions log prints Node v22.12.0 |
| [CCS-027](CCS-027-p0-ci-permissions.md) | `p0-ci-permissions` | Partial | confirm that the job log's GITHUB_TOKEN permissions are Contents/Metadata read only |
| [CCS-028](CCS-028-p0-ci-pin-actions.md) | `p0-ci-pin-actions` | Done |  |
| [CCS-029](CCS-029-p0-ci-release-secrets-env.md) | `p0-ci-release-secrets-env` | Partial | fork tag run produces .sig on all 3 OSes; maintainer confirms the signing secret is in canonical `.key` form |
| [CCS-030](CCS-030-p0-ci-updater-sig-fatal.md) | `p0-ci-updater-sig-fatal` | Done |  |
| [CCS-031](CCS-031-p0-ci-release-concurrency.md) | `p0-ci-release-concurrency` | Partial | fork test of publish concurrency when a tag is re-pushed |
| [CCS-032](CCS-032-p0-ci-prerelease-promotion.md) | `p0-ci-prerelease-promotion` | Partial | real promotion on a fork plus a `curl -sIL` check of the redirect |
| [CCS-033](CCS-033-p0-ci-release-hygiene.md) | `p0-ci-release-hygiene` | Partial | real tag run: SHA256SUMS present and `gh attestation verify` passes |
| [CCS-034](CCS-034-p0-dev-acceptance-process.md) | `p0-dev-acceptance-process` | Partial | make "Proxy fixture" a required check and create the `no-fixture-needed` label (GitHub settings); run actionlint |
| [CCS-035](CCS-035-p0-hotfix-deeplink-redaction.md) | `p0-hotfix-deeplink-redaction` | Done |  |
| [CCS-036](CCS-036-p0-hotfix-terminal-tempfiles.md) | `p0-hotfix-terminal-tempfiles` | Partial | manual desktop terminal check; compile the macOS/Windows cfg branches in CI |
| [CCS-037](CCS-037-p0-hotfix-sync-skip-session-log.md) | `p0-hotfix-sync-skip-session-log` | Done |  |
| [CCS-038](CCS-038-p0-hotfix-session-sync-blocking.md) | `p0-hotfix-session-sync-blocking` | Partial | manual UI responsiveness check with a large `~/.claude/projects` |
| [CCS-039](CCS-039-p0-hotfix-providerform-parse.md) | `p0-hotfix-providerform-parse` | Done |  |
| [CCS-040](CCS-040-p0-failure-threshold-const.md) | `p0-failure-threshold-const` | Done |  |
| [CCS-041](CCS-041-p0-unused-tower-http.md) | `p0-unused-tower-http` | Done |  |
| [CCS-042](CCS-042-p0-icon-scripts-cjs.md) | `p0-icon-scripts-cjs` | Done |  |
| [CCS-043](CCS-043-p0-ci-stale-workflow.md) | `p0-ci-stale-workflow` | Partial | `workflow_dispatch` debug-only run on GitHub |
| [CCS-044](CCS-044-p0-flatpak-manifest.md) | `p0-flatpak-manifest` | Done |  |
| [CCS-045](CCS-045-p0-docs-changelog-unreleased.md) | `p0-docs-changelog-unreleased` | Done |  |
| [CCS-046](CCS-046-p0-docs-readme-release-sync.md) | `p0-docs-readme-release-sync` | Done |  |
| [CCS-047](CCS-047-p0-docs-package-description.md) | `p0-docs-package-description` | Done |  |
| [CCS-048](CCS-048-p0-docs-root-clutter.md) | `p0-docs-root-clutter` | Done |  |

### What is done
- **Golden bridges.** 182 fixtures cover the 5 required bridges, plus `gemini__anthropic`. Every stream fixture is checked under byte-offset, UTF-8, CRLF, one-byte and seeded random splits. The inline proxy suite has grown to 1524 tests and all of them pass.
- **Cost, migrations, writers.** The cost golden table covers 8 apps x 3 semantics and is fully green: CCS-092 fixed the grokbuild backfill divergence and removed the `#[ignore]`. The migration harness runs 20 schema fixtures (v1 to v21) and 2 legacy `config.json` fixtures. Config-writer snapshots cover the 11 live writers, both OAuth token stores and the Codex state DB, each in LF, CRLF and BOM variants.
- **Test isolation.** `test_mutex` is gone. Integration tests run in parallel on a per-thread `TestEnv`. `proxy_e2e` has 37 scenarios.
- **CI and release gates.** CI runs clippy `--all-targets`, ESLint with a baseline ratchet, Prettier on `src/` and `tests/`, i18n parity and used-key checks, cargo-deny and pnpm audit, coverage reports, a build smoke and an MSRV check. Every workflow action is SHA-pinned. Release secrets are passed only through `env:`. A missing updater `.sig` fails the build. A tag cannot publish unless the reusable CI run and `check-versions` pass.
- **Hotfixes.** Deeplink logs and webview events no longer contain a raw `ccswitch://` URL. Terminal temp files are private, and the session sync runs off the main thread. The ProviderForm parse fix, the failure threshold, the tower-http removal, the icon scripts, the Flatpak manifest and the docs tickets are all Done.

### What remains, and why (epic stays Partial)
1. **Exit criterion 5, "green on 3 OSes" (unticked).** `proxy_e2e` and the rest of the suite have only been run on Linux. `ci.yml`'s `backend` matrix already runs `cargo test` on ubuntu-22.04, windows-latest and macos-latest, but no GitHub Actions run is visible on `ShuminFu/cc-switch` (`gh run list` returns nothing). To close this, push the branch or open a PR and confirm the macOS and Windows backend legs are green. The risks to watch are the allocation gate in `perf_baseline_non_streaming_request` and the 1.1 s half-open wait.
2. **Partial child tickets.** All 14 Partial children have their code and tests landed. Each one is waiting only on something this environment cannot do:
   - **A real GitHub Actions run:** CCS-019, CCS-026, CCS-027 and CCS-043.
   - **A fork tag or release with signing secrets on macOS and Windows runners:** CCS-023, CCS-029, CCS-031, CCS-032 and CCS-033.
   - **Repository settings a maintainer must change:** required status checks in branch protection (CCS-018, CCS-034) and required reviewers on the `release` environment (CCS-023).
   - **A manual desktop check:** CCS-036 (terminal temp files) and CCS-038 (sync responsiveness).
   - **Cross-OS CI:** CCS-014 and CCS-036.
   - **Housekeeping:** a `cargo update` to clear the baselined RUSTSEC ignores (CCS-018).
3. The ticked criteria 6 and 7 still need these live checks before anyone relies on them in a real release: the CCS-023 fork-tag skip test and the CCS-029 three-OS `.sig` run. They are ticked because the gating is structurally in place and covered by the `tests/ci/*` regression tests.

### Remaining after triage

- Blocked: Exit criterion 5 / CCS-014: proxy_e2e and the full suite green on macOS and Windows. Needs a windows-latest/macos-latest run of the ci.yml backend matrix, or real machines (cross-compiling to windows-gnu/apple fails on aws-lc-sys).
- Blocked: CCS-036 cfg-gated macOS/Windows branches; CCS-036 and CCS-038 interactive desktop checks. Need a CI matrix run or desktop GUI use (Linux substitutes are CCS-223 and CCS-224).
- Blocked: CCS-019, CCS-026, CCS-027, CCS-043 runtime checks (coverage artifacts, Node v22.12.0 in the log, GITHUB_TOKEN scopes, stale workflow dispatch). Need a real GitHub Actions run (CI triggers only on main/PRs; the repo has no Actions history).
- Blocked: CCS-023, CCS-029, CCS-031, CCS-032, CCS-033 release checks (red-tag skip, .sig on 3 OSes, publish concurrency, prerelease promotion, SHA256SUMS + attestation). Need a tag/release run on a fork with secrets.
- Blocked: CCS-018/CCS-034 required checks (cargo-deny, pnpm audit, Proxy fixture) and the `no-fixture-needed`/`fixtures` labels. Need a maintainer to change GitHub repo settings.
- Follow-up [CCS-213](../P0-safety-net/CCS-213-p0-ci-proxy-perf-artifact.md): Upload the proxy_e2e perf JSON as a CI artifact from the backend matrix.
- Follow-up [CCS-214](../P0-safety-net/CCS-214-p0-deps-cargo-update-deny-baseline.md): Refresh Cargo.lock and prune the cargo-deny advisory baseline.
- Follow-up [CCS-215](../P0-safety-net/CCS-215-p0-deps-zip-upgrade.md): Upgrade the direct zip dependency to a release that fixes RUSTSEC-2026-0367/0368/0369.
- Follow-up [CCS-217](../P0-safety-net/CCS-217-p0-ci-actionlint-shellcheck.md): Run actionlint with shellcheck on every workflow and release script, and commit its config.
- Follow-up [CCS-221](../P0-safety-net/CCS-221-p0-docs-ticket-evidence-refresh.md): Tick already-verified acceptance boxes and refresh stale evidence in Partial tickets.
- Follow-up [CCS-223](../P0-safety-net/CCS-223-p0-terminal-tempfile-headless-e2e.md): Headless Linux end-to-end check of private terminal temp files, plus temp-dir selection tests.
- Follow-up [CCS-224](../P0-safety-net/CCS-224-p0-usage-session-sync-responsiveness.md): Prove the UI stays responsive during a large session-log sync with a headless e2e spec.
