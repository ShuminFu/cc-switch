# CCS-001: [epic] P0 (v3.18): Safety net, real CI gates and zero-risk hotfixes

**Status:** Partial · **Kind:** epic · **Labels:** `epic`, `phase:P0`, `p0-critical`, `area:tests`

> **Ticket key:** `epic-p0` · **Phase:** P0 · **Priority:** p0-critical · **Effort:** XL
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Pin current behaviour in the areas that regress most (protocol bridges, cost accounting, migrations, live-config writers, proxy lifecycle) and make CI enforce what CONTRIBUTING.md claims. Ship a handful of hotfixes that need no harness.

## Exit criteria
- [x] At least 60 golden bridge fixtures across all 5 bridges (stream and non-stream), passing under every-offset, random, UTF-8 and CRLF chunk splitting; the ~1046 existing inline proxy tests still pass — *re-audit 2026-10-31: 182 `meta.toml` fixtures under `src-tauri/tests/fixtures/proxy/` (anthropic__openai_chat 39, anthropic__gemini 32, anthropic__responses 34, responses__chat 31, responses__anthropic 29, plus gemini__anthropic 17); split plans `byte-offset`/`utf8`/`crlf`/`random#N` in `proxy/providers/testkit.rs`. The inline suite keeps growing: the follow-up notes record full `cargo test --lib` runs of 2855-2869 passed, 0 failed, 3 ignored (CCS-230/231/236/244/245), and CCS-233/236 report `proxy_golden` unchanged and green.*
- [x] Cost golden table covers 8 apps x 3 semantics; the only failures are the known grokbuild backfill rows, marked #[ignore] — *re-audit: `proxy/usage/cost_golden.rs` still iterates `AppType::all()` (== 8) x LEGACY/TOTAL/FRESH and contains no `#[ignore]` (CCS-092 fixed grokbuild). CCS-232 only excluded the file from the match-app site counter; it did not change the table.*
- [x] Migration harness passes from at least 10 start versions plus the legacy config.json path (migrate_from_json) — *re-audit: 20 DB fixtures (`tests/fixtures/db/v1..v21.sql`, no v14) plus `tests/fixtures/legacy/config-v3.6-per-app-mcp.json` and `config-v3.7-unified-mcp.json`; CCS-214 (Cargo.lock refresh) re-ran `--test migrations` green.*
- [x] Config-writer snapshots exist for all 11 live writers plus the Copilot/Codex OAuth token stores and the Codex state DB; untouched bytes stay identical — *re-audit: `tests/fixtures/live/` still holds the 11 writers plus copilot_auth, codex_oauth, codex_state_db, codex_preserve_auth and `file_modes.snap`; the apps refactors (CCS-233, CCS-234, CCS-235) each re-ran `--test config_writers`: 19 passed, byte-identical.*
- [ ] test_mutex is removed, integration tests run in parallel, and proxy_e2e has at least 8 scenarios, green on 3 OSes — *re-audit: `grep -rn test_mutex src-tauri` finds nothing; `cargo test --test proxy_e2e` on 2026-10-31 (Linux x86_64, HEAD 673be73): 37 passed, 0 failed, 3.60 s. Still unticked: "green on 3 OSes". The ci.yml `backend` matrix (ubuntu-22.04, windows-latest, macos-latest) runs it and now uploads `proxy-e2e-perf-${{ matrix.os }}` (CCS-213), but `gh run list -R ShuminFu/cc-switch` is still empty.*
- [x] CI enforces clippy --all-targets, ESLint (baseline), Prettier on tests/, cargo-deny and i18n parity; a tag with no green CI cannot publish — *re-audit: ci.yml:148 `cargo clippy ... --all-targets -- -D warnings`; `pnpm lint` (`eslint . --max-warnings 0`) and `pnpm format:check` (`{src,tests}/**`) at ci.yml:65/68; `pnpm test:unit` includes the i18n parity and used-key tests; audit.yml runs cargo-deny-action, and its baseline was cut from 32 entries to 6 by CCS-214/215. release.yml: `release` needs `[ci, check-versions]`, `publish-release` needs `release`, `assemble-latest-json` needs `publish-release`; the only `always()` lines (675, 682) are debug/keychain-cleanup steps inside `release`. CCS-218 added executable regression tests (`tests/ci/releaseGate`, `releaseSigningKeyValidate`, `releaseSha256Sums`) and CCS-217 added actionlint with shellcheck. Required checks on `main` (CCS-018/034) and the live fork-tag test (CCS-023) are still open.*
- [x] Release pipeline: SHA-pinned actions, secrets passed via env:, missing updater .sig is fatal — *re-audit: `grep 'uses:' .github/workflows/*.yml` finds no non-40-hex ref apart from local `./` workflows (the new CCS-213 upload step reuses the pinned `upload-artifact@cf430e0…`); every `secrets.` in release.yml is a `NAME: ${{ secrets.X }}` mapping; no `TAURI_SIGNING_*` goes to `GITHUB_ENV`; macOS (release.yml:415), Windows (563-570) and Linux (630) `exit 1` on a missing or empty `.sig`. CCS-220 confirmed locally that a signed Linux build produces a valid `.sig`; macOS/Windows remain (CCS-029).*
- [x] No log line or webview event contains an unredacted ccswitch:// URL — *re-audit: `app_shell/run_events.rs` logs only `deeplink::redact_url_for_log(..)` (lines 51, 96) or parsed fields; `deeplink-error` carries `deeplink_error_payload` (`deeplink/redact.rs:41`, test `error_payload_does_not_contain_secret`); no new deeplink logging came in with the follow-ups.*

## Child tickets
p0-golden-testkit-chat-gemini, p0-golden-responses-fixtures, p0-cost-golden-table, p0-migration-harness, p0-config-writer-snapshots, p0-testenv-paths, p0-proxy-harness-e2e, p0-untested-module-tests, p0-ci-clippy-all-targets, p0-ci-eslint-prettier, p0-ci-cargo-deny, p0-ci-coverage-report, p0-ci-i18n-parity, p0-i18n-missing-keys, p0-i18n-zhtw-keys, p0-ci-release-gated, p0-ci-build-smoke, p0-ci-toolchain-msrv, p0-ci-node-version, p0-ci-permissions, p0-ci-pin-actions, p0-ci-release-secrets-env, p0-ci-updater-sig-fatal, p0-ci-release-concurrency, p0-ci-prerelease-promotion, p0-ci-release-hygiene, p0-dev-acceptance-process, p0-hotfix-deeplink-redaction, p0-hotfix-terminal-tempfiles, p0-hotfix-sync-skip-session-log, p0-hotfix-session-sync-blocking, p0-hotfix-providerform-parse, p0-failure-threshold-const, p0-unused-tower-http, p0-icon-scripts-cjs, p0-ci-stale-workflow, p0-flatpak-manifest, p0-docs-changelog-unreleased, p0-docs-readme-release-sync, p0-docs-package-description, p0-docs-root-clutter

## Epic closure audit

Re-audited 2026-10-31 on branch `claude/funny-carson-9c7j3e` at 673be73 (after the merge of the main-line follow-ups and the lane 2 follow-ups CCS-213..253). Commands run (read-only, Linux x86_64): `cargo test --test proxy_e2e` (37 passed, 0 failed), fixture counts under `src-tauri/tests/fixtures/`, `grep -rn test_mutex src-tauri`, greps over `.github/workflows/*.yml`, `src-tauri/src/app_shell/run_events.rs` and `src-tauri/src/deeplink/`, and `gh run list -R ShuminFu/cc-switch` (empty). Other test counts come from the follow-up ticket notes (full `--lib` runs of up to 2869 passed, 0 failed; `proxy_golden`, `config_writers` 19/19, `migrations`, `takeover_crash` 10/10 re-run by CCS-214/215/233/234/236). The 2026-10-30 audit measured `proxy_golden` 18/18, `migrations` 14/14, `config_writers` 19/19, `test_env_isolation` 2/2 and `cost_golden` 7/7 with 0 ignored.

### Child tickets (41: 28 Done, 13 Partial)

| Ticket | Key | Status | What remains (Partial only) |
| --- | --- | --- | --- |
| [CCS-008](CCS-008-p0-golden-testkit-chat-gemini.md) | `p0-golden-testkit-chat-gemini` | Done |  |
| [CCS-009](CCS-009-p0-golden-responses-fixtures.md) | `p0-golden-responses-fixtures` | Done |  |
| [CCS-010](CCS-010-p0-cost-golden-table.md) | `p0-cost-golden-table` | Done |  |
| [CCS-011](CCS-011-p0-migration-harness.md) | `p0-migration-harness` | Done |  |
| [CCS-012](CCS-012-p0-config-writer-snapshots.md) | `p0-config-writer-snapshots` | Done |  |
| [CCS-013](CCS-013-p0-testenv-paths.md) | `p0-testenv-paths` | Done |  |
| [CCS-014](CCS-014-p0-proxy-harness-e2e.md) | `p0-proxy-harness-e2e` | Partial | a green macOS/Windows backend-matrix run; confirm the perf artifact uploads (step added by CCS-213) |
| [CCS-015](CCS-015-p0-untested-module-tests.md) | `p0-untested-module-tests` | Done |  |
| [CCS-016](CCS-016-p0-ci-clippy-all-targets.md) | `p0-ci-clippy-all-targets` | Done |  |
| [CCS-017](CCS-017-p0-ci-eslint-prettier.md) | `p0-ci-eslint-prettier` | Done |  |
| [CCS-018](CCS-018-p0-ci-cargo-deny.md) | `p0-ci-cargo-deny` | Partial | make `cargo-deny` and `pnpm audit` required checks on `main` (GitHub setting); the `cargo update` and zip work is done (CCS-214, CCS-215) |
| [CCS-019](CCS-019-p0-ci-coverage-report.md) | `p0-ci-coverage-report` | Partial | a real PR run showing both coverage artifacts and the per-directory summaries (baseline re-measured by CCS-222) |
| [CCS-020](CCS-020-p0-ci-i18n-parity.md) | `p0-ci-i18n-parity` | Done |  |
| [CCS-021](CCS-021-p0-i18n-missing-keys.md) | `p0-i18n-missing-keys` | Done |  |
| [CCS-022](CCS-022-p0-i18n-zhtw-keys.md) | `p0-i18n-zhtw-keys` | Done |  |
| [CCS-023](CCS-023-p0-ci-release-gated.md) | `p0-ci-release-gated` | Partial | fork test: a `v*` tag on a red commit skips release; required reviewers on the `release` environment (GitHub setting) |
| [CCS-024](CCS-024-p0-ci-build-smoke.md) | `p0-ci-build-smoke` | Done |  |
| [CCS-025](CCS-025-p0-ci-toolchain-msrv.md) | `p0-ci-toolchain-msrv` | Done |  |
| [CCS-026](CCS-026-p0-ci-node-version.md) | `p0-ci-node-version` | Partial | an Actions log that prints v22.12.0 (CCS-219 now asserts it in every job) |
| [CCS-027](CCS-027-p0-ci-permissions.md) | `p0-ci-permissions` | Partial | an Actions "Set up job" log showing only Contents/Metadata read |
| [CCS-028](CCS-028-p0-ci-pin-actions.md) | `p0-ci-pin-actions` | Done |  |
| [CCS-029](CCS-029-p0-ci-release-secrets-env.md) | `p0-ci-release-secrets-env` | Partial | a fork tag run producing `.sig` on macOS and Windows (Linux verified by CCS-220); a maintainer confirms the signing secret is in canonical `.key` form |
| [CCS-030](CCS-030-p0-ci-updater-sig-fatal.md) | `p0-ci-updater-sig-fatal` | Done |  |
| [CCS-031](CCS-031-p0-ci-release-concurrency.md) | `p0-ci-release-concurrency` | Partial | fork test of publish concurrency when a tag is re-pushed |
| [CCS-032](CCS-032-p0-ci-prerelease-promotion.md) | `p0-ci-prerelease-promotion` | Partial | a real promotion on a fork plus the `curl -sIL` redirect check |
| [CCS-033](CCS-033-p0-ci-release-hygiene.md) | `p0-ci-release-hygiene` | Partial | a signed tag run: SHA256SUMS present and `gh attestation verify` passes |
| [CCS-034](CCS-034-p0-dev-acceptance-process.md) | `p0-dev-acceptance-process` | Partial | make "Proxy fixture required" a required check; create the `no-fixture-needed` and `fixtures` labels (GitHub settings) |
| [CCS-035](CCS-035-p0-hotfix-deeplink-redaction.md) | `p0-hotfix-deeplink-redaction` | Done |  |
| [CCS-036](CCS-036-p0-hotfix-terminal-tempfiles.md) | `p0-hotfix-terminal-tempfiles` | Partial | compile/run the macOS/Windows cfg branches in the CI matrix; a manual check in a real desktop terminal (Linux behaviour covered by CCS-223) |
| [CCS-037](CCS-037-p0-hotfix-sync-skip-session-log.md) | `p0-hotfix-sync-skip-session-log` | Done |  |
| [CCS-038](CCS-038-p0-hotfix-session-sync-blocking.md) | `p0-hotfix-session-sync-blocking` | Done | (was Partial; closed by the CCS-224 headless responsiveness spec) |
| [CCS-039](CCS-039-p0-hotfix-providerform-parse.md) | `p0-hotfix-providerform-parse` | Done |  |
| [CCS-040](CCS-040-p0-failure-threshold-const.md) | `p0-failure-threshold-const` | Done |  |
| [CCS-041](CCS-041-p0-unused-tower-http.md) | `p0-unused-tower-http` | Done |  |
| [CCS-042](CCS-042-p0-icon-scripts-cjs.md) | `p0-icon-scripts-cjs` | Done |  |
| [CCS-043](CCS-043-p0-ci-stale-workflow.md) | `p0-ci-stale-workflow` | Partial | a `workflow_dispatch` run with `debug-only: true` on GitHub |
| [CCS-044](CCS-044-p0-flatpak-manifest.md) | `p0-flatpak-manifest` | Done |  |
| [CCS-045](CCS-045-p0-docs-changelog-unreleased.md) | `p0-docs-changelog-unreleased` | Done |  |
| [CCS-046](CCS-046-p0-docs-readme-release-sync.md) | `p0-docs-readme-release-sync` | Done |  |
| [CCS-047](CCS-047-p0-docs-package-description.md) | `p0-docs-package-description` | Done |  |
| [CCS-048](CCS-048-p0-docs-root-clutter.md) | `p0-docs-root-clutter` | Done |  |

### P0 follow-up tickets (12: 11 Done, 1 Partial)

| Ticket | Key | Status | Notes |
| --- | --- | --- | --- |
| [CCS-213](CCS-213-p0-ci-proxy-perf-artifact.md) | `p0-ci-proxy-perf-artifact` | Partial | the upload step is in ci.yml; needs a real backend-matrix run to confirm the upload |
| [CCS-214](CCS-214-p0-deps-cargo-update-deny-baseline.md) | `p0-deps-cargo-update-deny-baseline` | Done |  |
| [CCS-215](CCS-215-p0-deps-zip-upgrade.md) | `p0-deps-zip-upgrade` | Done | (3 ignores stay for the Windows-only zip 4 copy in tauri-plugin-updater; dropping them needs a tauri 2.12 / MSRV 1.90 bump) |
| [CCS-216](CCS-216-p0-deps-vitest-upgrade.md) | `p0-deps-vitest-upgrade` | Done |  |
| [CCS-217](CCS-217-p0-ci-actionlint-shellcheck.md) | `p0-ci-actionlint-shellcheck` | Done |  |
| [CCS-218](CCS-218-p0-ci-release-regression-tests.md) | `p0-ci-release-regression-tests` | Done |  |
| [CCS-219](CCS-219-p0-ci-node-version-assert.md) | `p0-ci-node-version-assert` | Done |  |
| [CCS-220](CCS-220-p0-release-linux-sig-local.md) | `p0-release-linux-sig-local` | Done |  |
| [CCS-221](CCS-221-p0-docs-ticket-evidence-refresh.md) | `p0-docs-ticket-evidence-refresh` | Done |  |
| [CCS-222](CCS-222-p0-coverage-remeasure-baseline.md) | `p0-coverage-remeasure-baseline` | Done |  |
| [CCS-223](CCS-223-p0-terminal-tempfile-headless-e2e.md) | `p0-terminal-tempfile-headless-e2e` | Done |  |
| [CCS-224](CCS-224-p0-usage-session-sync-responsiveness.md) | `p0-usage-session-sync-responsiveness` | Done |  |

Follow-ups filed under other phases (CCS-225..253, for example CCS-227 IPv6 guard, CCS-230/231 proxy forwarder size and p50, CCS-232..236 apps refactors, CCS-242 coverage gate) belong to the P1 to P5 and F epics and are audited there. None of them changes a P0 exit criterion.

### What is done
- **Golden bridges.** 182 fixtures cover the 5 required bridges plus `gemini__anthropic`, under byte-offset, UTF-8, CRLF, one-byte and seeded random splits. The inline suite has grown well past the ~1046 baseline and stays green through the proxy refactors (CCS-230, CCS-231, CCS-245).
- **Cost, migrations, writers.** The cost golden table (8 apps x 3 semantics) is fully green with no `#[ignore]`. The migration harness covers 20 schema versions plus 2 legacy `config.json` files. Config-writer snapshots cover the 11 live writers, both OAuth token stores and the Codex state DB in LF/CRLF/BOM, and survived the apps refactors byte-identical.
- **Test isolation.** `test_mutex` is gone; integration tests run in parallel on a per-thread `TestEnv`; `proxy_e2e` has 37 scenarios, all green on Linux, and the backend matrix now keeps its perf JSON as an artifact (CCS-213).
- **CI and release gates.** clippy `--all-targets`, ESLint `--max-warnings 0` with a baseline ratchet, Prettier on `src/` and `tests/`, i18n parity, cargo-deny (baseline cut from 32 to 6 entries by CCS-214/215) and pnpm audit (vitest upgraded and the 4 GHSA ignores dropped by CCS-216), coverage, build smoke, MSRV, actionlint+shellcheck (CCS-217) and a Node version assertion in every job (CCS-219). Every action is SHA-pinned, release secrets go only through `env:`, a missing `.sig` fails all three packaging legs, and the release chain is gated on CI plus `check-versions`. 15 `tests/ci/*` guard files, including executable release-gate, signing-key and SHA256SUMS tests (CCS-218), pin all of this. A signed Linux build was verified locally (CCS-220).
- **Hotfixes.** Deeplink redaction, private terminal temp files (Linux headless e2e by CCS-223) and off-main-thread session sync (CCS-038, now Done via the CCS-224 headless responsiveness spec) are in, along with the other hotfix and docs tickets.

### What remains, and exactly what it needs (epic stays Partial)
1. **Exit criterion 5, "green on 3 OSes" (the only unticked criterion).** Needs one GitHub Actions run of the ci.yml `backend` matrix on windows-latest and macos-latest (push the branch to `main` or open a PR; the repo has no Actions history). Check `proxy_e2e` there, especially the 3x allocation gate in `perf_baseline_non_streaming_request` and the 1.1 s half-open wait, and that the `proxy-e2e-perf-<os>` artifacts upload (CCS-014, CCS-213). The same run compiles the cfg-gated macOS/Windows terminal code (CCS-036). Cross-compiling locally is not an option: aws-lc-sys fails for windows-gnu and apple targets.
2. **A real GitHub Actions run (PR or `main`):** CCS-019 (coverage artifacts and summaries), CCS-026 (log prints v22.12.0), CCS-027 (GITHUB_TOKEN shows Contents/Metadata read only), CCS-043 (`workflow_dispatch` of Close Stale Issues with `debug-only: true`), CCS-034 (the proxy-fixture workflow on a real PR).
3. **A signed tag run on a fork with the release secrets and macOS/Windows runners:** CCS-023 (red-commit tag skips release, publish-release and assemble-latest-json), CCS-029 (`.sig` on macOS and Windows), CCS-031 (re-pushed tag waits in the publish concurrency group), CCS-032 (promotion and the `curl -sIL` latest.json redirect), CCS-033 (SHA256SUMS present, `gh attestation verify` passes).
4. **GitHub settings a maintainer must change:** required status checks on `main` for cargo-deny, pnpm audit and "Proxy fixture required" (CCS-018, CCS-034); the `no-fixture-needed` and `fixtures` labels (CCS-034); required reviewers on the `release` environment (CCS-023); confirming that `TAURI_SIGNING_PRIVATE_KEY` holds the canonical single-line `.key` form (CCS-029).
5. **A human GUI check:** CCS-036, running claude from a real desktop terminal (gnome-terminal/konsole) to see `-rw-------` temp files that are removed on exit.

Ticked criteria 6 and 7 rest on the structural gating and the `tests/ci/*` guards; items 3 and 4 above are the live confirmation to do before relying on them for a real release.

Housekeeping noted while auditing (no status change made here): a few Partial children still have acceptance boxes unticked that are now locally verified. CCS-026 AC1: the only `node-version` matches outside `node-version-file` are the CCS-219 assertion lines, which read `.node-version`. CCS-029 AC1/AC2: secrets only in `env:` mappings and no `TAURI_SIGNING_*` in `GITHUB_ENV`. CCS-032 AC1: `docs/dev/RELEASING.md` exists and CONTRIBUTING.md:101 links to it.
