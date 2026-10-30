# Ticket index

Tickets generated from the full-codebase review and the [next-phase plan](../next-phase-plan.md). Every defect ticket comes from a review finding that an independent verifier checked against the code (99 of 101 findings held up; 2 were refuted and dropped).

Status values: `Open`, `In progress`, `Done`, `Blocked` (with reason).

## P0: [P0 (v3.18): Safety net, real CI gates and zero-risk hotfixes](P0-safety-net/CCS-001-epic-p0.md)

| ID | Kind | Pri | Effort | Title | Depends on | Status |
|---|---|---|---|---|---|---|
| [CCS-008](P0-safety-net/CCS-008-p0-golden-testkit-chat-gemini.md) | chore | p1 | M | [proxy-tests] Add golden bridge testkit with chunk-split fuzzing, plus Chat and Gemini fixtures |  | Done |
| [CCS-009](P0-safety-net/CCS-009-p0-golden-responses-fixtures.md) | chore | p1 | M | [proxy-tests] Add golden fixtures for the Responses bridges and recent regression commits | CCS-008 | Done |
| [CCS-010](P0-safety-net/CCS-010-p0-cost-golden-table.md) | chore | p1 | S | [usage] Add a cost golden table that runs every cost path |  | Done |
| [CCS-011](P0-safety-net/CCS-011-p0-migration-harness.md) | chore | p1 | M | [db] Add a migration equivalence harness from every released schema and from legacy config.json |  | Done |
| [CCS-012](P0-safety-net/CCS-012-p0-config-writer-snapshots.md) | chore | p1 | M | [apps] Add round-trip snapshots for all live-config writers, OAuth token stores and the Codex state DB |  | Done |
| [CCS-013](P0-safety-net/CCS-013-p0-testenv-paths.md) | refactor | p1 | L | [tests] Introduce a hermetic TestEnv with injected Paths and remove the global test_mutex |  | Done |
| [CCS-014](P0-safety-net/CCS-014-p0-proxy-harness-e2e.md) | chore | p1 | L | [proxy-tests] Add ProxyHarness with a mock upstream and an end-to-end proxy suite with baselines | CCS-013 | Partial |
| [CCS-015](P0-safety-net/CCS-015-p0-untested-module-tests.md) | chore | p2 | S | [tests] Add unit tests for untested proxy helpers, failover switch and balance parsing |  | Done |
| [CCS-016](P0-safety-net/CCS-016-p0-ci-clippy-all-targets.md) | chore | p2 | S | [ci] Run clippy with --all-targets |  | Done |
| [CCS-017](P0-safety-net/CCS-017-p0-ci-eslint-prettier.md) | chore | p2 | M | [ci] Add ESLint with a baseline and extend Prettier to tests/ |  | Done |
| [CCS-018](P0-safety-net/CCS-018-p0-ci-cargo-deny.md) | chore | p2 | S | [ci] Gate on dependency advisories with cargo-deny and pnpm audit |  | Partial |
| [CCS-019](P0-safety-net/CCS-019-p0-ci-coverage-report.md) | chore | p3 | S | [ci] Publish report-only coverage from cargo-llvm-cov and vitest |  | Partial |
| [CCS-020](P0-safety-net/CCS-020-p0-ci-i18n-parity.md) | chore | p2 | S | [i18n] Add a locale parity and used-key check to CI |  | Done |
| [CCS-021](P0-safety-net/CCS-021-p0-i18n-missing-keys.md) | bug | p2 | S | [i18n] Add 10 keys that are used in code but missing from every locale |  | Done |
| [CCS-022](P0-safety-net/CCS-022-p0-i18n-zhtw-keys.md) | bug | p3 | S | [i18n] Translate the 30 missing zh-TW keys and drop 2 unused zh-TW-only keys |  | Done |
| [CCS-023](P0-safety-net/CCS-023-p0-ci-release-gated.md) | chore | p1 | S | [ci] Require green CI on the tag SHA before release builds |  | Partial |
| [CCS-024](P0-safety-net/CCS-024-p0-ci-build-smoke.md) | chore | p2 | S | [ci] Add a vite production build and a Linux tauri build smoke to PR CI |  | Done |
| [CCS-025](P0-safety-net/CCS-025-p0-ci-toolchain-msrv.md) | chore | p2 | S | [ci] Use rust-toolchain.toml as the single toolchain source and fix the false MSRV |  | Done |
| [CCS-026](P0-safety-net/CCS-026-p0-ci-node-version.md) | chore | p3 | S | [ci] Read the Node version from .node-version in CI and release |  | Partial |
| [CCS-027](P0-safety-net/CCS-027-p0-ci-permissions.md) | security | p3 | S | [ci] Add a minimal permissions block to ci.yml |  | Partial |
| [CCS-028](P0-safety-net/CCS-028-p0-ci-pin-actions.md) | security | p1 | S | [ci] Pin every GitHub Action to a commit SHA |  | Done |
| [CCS-029](P0-safety-net/CCS-029-p0-ci-release-secrets-env.md) | security | p1 | S | [ci] Pass release secrets via env: and stop exporting signing material to GITHUB_ENV | CCS-028 | Partial |
| [CCS-030](P0-safety-net/CCS-030-p0-ci-updater-sig-fatal.md) | bug | p1 | S | [ci] Fail the release when an updater artifact or signature is missing and validate latest.json |  | Done |
| [CCS-031](P0-safety-net/CCS-031-p0-ci-release-concurrency.md) | chore | p3 | S | [ci] Never cancel an in-progress release publish |  | Partial |
| [CCS-032](P0-safety-net/CCS-032-p0-ci-prerelease-promotion.md) | chore | p3 | S | [ci] Document or automate promoting a prerelease to latest for the updater |  | Partial |
| [CCS-033](P0-safety-net/CCS-033-p0-ci-release-hygiene.md) | chore | p3 | M | [ci] Add rust-cache, a version consistency check, provenance attestation and SHA256SUMS | CCS-028 | Partial |
| [CCS-034](P0-safety-net/CCS-034-p0-dev-acceptance-process.md) | chore | p2 | S | [process] Add the ACCEPTANCE playbook, the fixture-required PR rule and the fix-commit KPI | CCS-008 | Partial |
| [CCS-035](P0-safety-net/CCS-035-p0-hotfix-deeplink-redaction.md) | security | p1 | S | [deeplink] Stop leaking raw deeplink URLs to logs and to the webview console |  | Done |
| [CCS-036](P0-safety-net/CCS-036-p0-hotfix-terminal-tempfiles.md) | security | p1 | S | [terminal] Write provider terminal config and launcher scripts as private, uniquely named temp files |  | Partial |
| [CCS-037](P0-safety-net/CCS-037-p0-hotfix-sync-skip-session-log.md) | bug | p2 | S | [sync] Preserve local session_log_sync cursors on WebDAV/S3 import |  | Done |
| [CCS-038](P0-safety-net/CCS-038-p0-hotfix-session-sync-blocking.md) | bug | p1 | S | [usage] Move session-log sync off tokio workers and off the main thread |  | Partial |
| [CCS-039](P0-safety-net/CCS-039-p0-hotfix-providerform-parse.md) | bug | p0 | S | [provider-form] Block save when Codex or Gemini config fails to parse instead of silently saving the initial config |  | Done |
| [CCS-040](P0-safety-net/CCS-040-p0-failure-threshold-const.md) | bug | p3 | S | [proxy] Use one shared default for the circuit-breaker failure_threshold |  | Done |
| [CCS-041](P0-safety-net/CCS-041-p0-unused-tower-http.md) | chore | p3 | S | [deps] Remove the unused tower-http 0.5 dependency |  | Done |
| [CCS-042](P0-safety-net/CCS-042-p0-icon-scripts-cjs.md) | bug | p3 | S | [tooling] Make the icon extract and filter scripts runnable under type: module |  | Done |
| [CCS-043](P0-safety-net/CCS-043-p0-ci-stale-workflow.md) | bug | p3 | S | [ci] Remove the invalid stale.yml input and implement a real popular-issue exemption |  | Partial |
| [CCS-044](P0-safety-net/CCS-044-p0-flatpak-manifest.md) | chore | p3 | M | [packaging] Update the Flatpak manifest and either build it in CI or stop installing flatpak tools |  | Done |
| [CCS-045](P0-safety-net/CCS-045-p0-docs-changelog-unreleased.md) | chore | p3 | S | [docs] Add an [Unreleased] CHANGELOG section that lists Grok Build and other post-3.17 changes |  | Done |
| [CCS-046](P0-safety-net/CCS-046-p0-docs-readme-release-sync.md) | chore | p3 | S | [docs] Update the README release sections: release-notes links and Linux asset names |  | Done |
| [CCS-047](P0-safety-net/CCS-047-p0-docs-package-description.md) | chore | p3 | S | [docs] Update the package description to cover all supported tools |  | Done |
| [CCS-048](P0-safety-net/CCS-048-p0-docs-root-clutter.md) | chore | p3 | S | [docs] Move the session-manager PRD into docs/ and leave the Pages-served deplink.html alone |  | Done |
| [CCS-213](P0-safety-net/CCS-213-p0-ci-proxy-perf-artifact.md) | chore | p3 | S | [ci] Upload the proxy_e2e perf JSON as a CI artifact from the backend matrix |  | Open |
| [CCS-214](P0-safety-net/CCS-214-p0-deps-cargo-update-deny-baseline.md) | security | p2 | M | [deps] Refresh Cargo.lock and prune the cargo-deny advisory baseline |  | Open |
| [CCS-215](P0-safety-net/CCS-215-p0-deps-zip-upgrade.md) | security | p2 | M | [deps] Upgrade the direct zip dependency to a release that fixes RUSTSEC-2026-0367/0368/0369 | CCS-214 | Open |
| [CCS-216](P0-safety-net/CCS-216-p0-deps-vitest-upgrade.md) | security | p2 | M | [deps] Upgrade vitest 2.x to >=3.2.6 and drop the 4 ignored GHSAs from pnpm audit |  | Open |
| [CCS-217](P0-safety-net/CCS-217-p0-ci-actionlint-shellcheck.md) | chore | p3 | S | [ci] Run actionlint with shellcheck on every workflow and release script, and commit its config |  | Open |
| [CCS-218](P0-safety-net/CCS-218-p0-ci-release-regression-tests.md) | chore | p2 | M | [ci-tests] Add executable regression tests for the release gate, signing-key validation and SHA256SUMS |  | Open |
| [CCS-219](P0-safety-net/CCS-219-p0-ci-node-version-assert.md) | chore | p3 | S | [ci] Assert the resolved Node version after every setup-node step |  | Open |
| [CCS-220](P0-safety-net/CCS-220-p0-release-linux-sig-local.md) | chore | p3 | M | [release] Verify locally that a signed Linux build still produces a valid updater .sig | CCS-218 | Open |
| [CCS-221](P0-safety-net/CCS-221-p0-docs-ticket-evidence-refresh.md) | chore | p3 | S | [docs] Tick already-verified acceptance boxes and refresh stale evidence in Partial tickets |  | Open |
| [CCS-222](P0-safety-net/CCS-222-p0-coverage-remeasure-baseline.md) | chore | p3 | M | [coverage] Re-measure the coverage baseline on a clean tree and record proxy/ and database/ numbers |  | Open |
| [CCS-223](P0-safety-net/CCS-223-p0-terminal-tempfile-headless-e2e.md) | security | p2 | M | [terminal] Headless Linux end-to-end check of private terminal temp files, plus temp-dir selection tests |  | Open |
| [CCS-224](P0-safety-net/CCS-224-p0-usage-session-sync-responsiveness.md) | chore | p3 | M | [usage] Prove the UI stays responsive during a large session-log sync with a headless e2e spec |  | Open |

## P1: [P1 (v3.19): Trust release with security and correctness fixes, no structural change](P1-trust-release/CCS-002-epic-p1.md)

| ID | Kind | Pri | Effort | Title | Depends on | Status |
|---|---|---|---|---|---|---|
| [CCS-049](P1-trust-release/CCS-049-p1-proxy-inbound-auth.md) | security | p0 | M | [proxy] Require a per-install token and reject foreign Host, Origin and non-JSON requests | CCS-014, CCS-012 | Done |
| [CCS-050](P1-trust-release/CCS-050-p1-proxy-body-limit.md) | bug | p3 | S | [proxy] Enforce the request body limit and return 400 for malformed JSON |  | Done |
| [CCS-051](P1-trust-release/CCS-051-p1-proxy-halfopen-permit.md) | bug | p1 | M | [proxy] Release the HalfOpen probe slot when a request is cancelled | CCS-014 | Done |
| [CCS-052](P1-trust-release/CCS-052-p1-proxy-raw-send-no-resend.md) | bug | p1 | S | [proxy] Fall back from the raw header-case path only on pre-write errors |  | Done |
| [CCS-053](P1-trust-release/CCS-053-p1-proxy-config-rollback.md) | bug | p1 | S | [proxy] Roll back the server and DB config when a port or address change fails to bind | CCS-054 | Done |
| [CCS-054](P1-trust-release/CCS-054-p1-proxy-lifecycle-lock.md) | bug | p2 | S | [proxy] Serialize proxy start/stop with a lifecycle mutex and stop holding the server lock across shutdown |  | Done |
| [CCS-055](P1-trust-release/CCS-055-p1-proxy-first-byte-timeout.md) | bug | p2 | S | [proxy] Apply the streaming first-byte timeout on the native-Claude raw path | CCS-052 | Done |
| [CCS-056](P1-trust-release/CCS-056-p1-proxy-raw-body-timeout.md) | bug | p3 | S | [proxy] Bound non-streaming body reads on the raw path when failover is off |  | Done |
| [CCS-057](P1-trust-release/CCS-057-p1-proxy-auth-guard-refresh.md) | bug | p3 | S | [proxy] Stop holding the Copilot/Codex auth read guard across network token refresh |  | Done |
| [CCS-058](P1-trust-release/CCS-058-p1-proxy-mask-url.md) | security | p3 | S | [proxy] Make mask_url panic-free and never echo credentials from unparseable proxy URLs |  | Done |
| [CCS-059](P1-trust-release/CCS-059-p1-proxy-connect-status.md) | bug | p3 | S | [proxy] Parse the CONNECT status line instead of substring-matching " 200 " |  | Done |
| [CCS-060](P1-trust-release/CCS-060-p1-proxy-connect-userinfo.md) | bug | p3 | S | [proxy] Percent-decode proxy credentials before building Proxy-Authorization |  | Done |
| [CCS-061](P1-trust-release/CCS-061-p1-proxy-connect-ipv6.md) | bug | p3 | S | [proxy] Strip IPv6 brackets before TCP connect and TLS server-name parsing |  | Partial |
| [CCS-062](P1-trust-release/CCS-062-p1-bridge-tool-result.md) | bug | p2 | S | [bridge] Convert array tool_result content properly in Anthropic->OpenAI Chat (text, images, is_error) | CCS-008 | Done |
| [CCS-063](P1-trust-release/CCS-063-p1-bridge-image-document.md) | bug | p2 | S | [bridge] Handle URL-source images and document blocks in Anthropic->OpenAI Chat | CCS-008 | Done |
| [CCS-064](P1-trust-release/CCS-064-p1-bridge-chat-stream-terminal.md) | bug | p2 | M | [bridge] Port the terminal state machine to the Chat->Anthropic stream converter | CCS-008 | Done |
| [CCS-065](P1-trust-release/CCS-065-p1-bridge-stop-reason-tool-use.md) | bug | p2 | S | [bridge] Map finish_reason stop to tool_use when tool calls were emitted | CCS-008 | Done |
| [CCS-066](P1-trust-release/CCS-066-p1-bridge-gemini-delta.md) | bug | p3 | S | [bridge] Decide Gemini cumulative vs incremental mode once per stream | CCS-008 | Done |
| [CCS-067](P1-trust-release/CCS-067-p1-usage-script-sandbox.md) | security | p1 | M | [usage-script] Run usage scripts with interrupt, memory and stack limits off the tokio workers |  | Done |
| [CCS-068](P1-trust-release/CCS-068-p1-usage-script-placeholder-escape.md) | bug | p3 | S | [usage-script] Escape placeholder values for a JS string context |  | Done |
| [CCS-069](P1-trust-release/CCS-069-p1-deeplink-script-disabled.md) | security | p1 | S | [deeplink] Store deeplink-delivered usage scripts disabled until the user reviews and enables them | CCS-067 | Done |
| [CCS-070](P1-trust-release/CCS-070-p1-deeplink-env-allowlist.md) | security | p2 | S | [deeplink] Allow-list env keys imported from Claude deeplinks and flag process-hijacking variables |  | Done |
| [CCS-071](P1-trust-release/CCS-071-p1-deeplink-dialog-guard.md) | security | p3 | S | [deeplink] Do not replace the request under an open confirm dialog |  | Done |
| [CCS-072](P1-trust-release/CCS-072-p1-deeplink-mask-value.md) | security | p3 | S | [deeplink] Use one masking helper that shows at most 4 characters |  | Done |
| [CCS-073](P1-trust-release/CCS-073-p1-deeplink-confirm-diff.md) | feature | p2 | M | [deeplink] Show a full import preview with a risk diff in the confirm dialog | CCS-070, CCS-069 | Done |
| [CCS-074](P1-trust-release/CCS-074-p1-ipc-session-terminal.md) | security | p1 | S | [ipc] Make launch_session_terminal build its resume command on the backend |  | Done |
| [CCS-075](P1-trust-release/CCS-075-p1-ipc-path-validation.md) | security | p1 | M | [ipc] Validate path arguments of privileged commands against allowed roots or dialog-issued tokens |  | Done |
| [CCS-076](P1-trust-release/CCS-076-p1-session-load-messages-roots.md) | security | p3 | S | [sessions] Apply the provider-root check to load_messages and the sqlite delete paths |  | Done |
| [CCS-077](P1-trust-release/CCS-077-p1-csp-tighten.md) | security | p3 | S | [security] Narrow the CSP connect-src and img-src |  | Partial |
| [CCS-078](P1-trust-release/CCS-078-p1-asset-protocol-remove.md) | security | p3 | S | [security] Remove the unused asset protocol and protocol-asset feature |  | Partial |
| [CCS-079](P1-trust-release/CCS-079-p1-atomic-write-durable.md) | bug | p1 | S | [config] Make atomic_write durable, Windows-safe and free of leftover temp files | CCS-012 | Partial |
| [CCS-080](P1-trust-release/CCS-080-p1-atomic-write-perms.md) | security | p1 | S | [config] Create new files written by atomic_write with 0600 permissions | CCS-079 | Done |
| [CCS-081](P1-trust-release/CCS-081-p1-atomic-write-symlink.md) | bug | p2 | S | [config] Write through symlinked config files instead of replacing the link | CCS-079 | Done |
| [CCS-082](P1-trust-release/CCS-082-p1-route-writes-atomic-lint.md) | refactor | p2 | S | [config] Route the remaining config writers through atomic_write and ban std::fs::write | CCS-080, CCS-012 | Done |
| [CCS-083](P1-trust-release/CCS-083-p1-oauth-token-store-hardening.md) | security | p2 | M | [auth] Harden the Copilot and Codex OAuth token stores | CCS-080 | Partial |
| [CCS-084](P1-trust-release/CCS-084-p1-settings-atomic-save.md) | bug | p1 | S | [settings] Save settings.json atomically and back up a corrupt file instead of silently resetting | CCS-080 | Partial |
| [CCS-085](P1-trust-release/CCS-085-p1-settings-save-race.md) | bug | p2 | S | [settings] Hold the settings write lock across merge, save and swap | CCS-084 | Done |
| [CCS-086](P1-trust-release/CCS-086-p1-sql-import-sandbox.md) | security | p1 | S | [db] Block ATTACH, VACUUM INTO and unsafe statements during SQL import and sync restore |  | Done |
| [CCS-087](P1-trust-release/CCS-087-p1-restore-backup-temp-first.md) | bug | p2 | S | [db] Restore backups into a temp DB and validate before overwriting the main DB | CCS-011 | Done |
| [CCS-088](P1-trust-release/CCS-088-p1-backup-filename-validation.md) | security | p3 | S | [db] Validate backup file names so drive-relative Windows paths cannot escape the backups dir |  | Done |
| [CCS-089](P1-trust-release/CCS-089-p1-db-premigration-backup.md) | bug | p1 | S | [db] Take a consistent pre-migration backup before any DDL, including v0 and JSON upgrades | CCS-011 | Done |
| [CCS-090](P1-trust-release/CCS-090-p1-db-wal-busy-timeout.md) | refactor | p2 | S | [db] Enable WAL, synchronous=NORMAL and busy_timeout on local DB paths | CCS-089 | Done |
| [CCS-091](P1-trust-release/CCS-091-p1-db-compat-policy.md) | chore | p1 | M | [db] Define and test the downgrade and mixed-version sync policy | CCS-089 | Done |
| [CCS-092](P1-trust-release/CCS-092-p1-usage-input-semantics.md) | bug | p1 | S | [usage] Make AppType::input_semantics() the single source and fix the grokbuild backfill cost | CCS-010 | Done |
| [CCS-093](P1-trust-release/CCS-093-p1-usage-session-importer-semantics.md) | bug | p2 | S | [usage] Persist input_token_semantics and pricing_model from session importers, and fix the dead Gemini UPSERT | CCS-092 | Done |
| [CCS-094](P1-trust-release/CCS-094-p1-pricing-no-reseed.md) | bug | p2 | S | [pricing] Stop reseeding built-in prices on every read so deleted prices stay deleted |  | Done |
| [CCS-095](P1-trust-release/CCS-095-p1-spend-limits-correct.md) | bug | p2 | S | [usage] Fix spend-limit windows, add an index and surface DB errors | CCS-091 | Done |
| [CCS-096](P1-trust-release/CCS-096-p1-session-sync-partial-line.md) | bug | p2 | S | [usage] Advance the session-log offset only past complete newline-terminated lines |  | Done |
| [CCS-097](P1-trust-release/CCS-097-p1-session-sync-final-entry.md) | bug | p2 | S | [usage] Upgrade imported partial streaming snapshots when the final session entry arrives | CCS-096 | Done |
| [CCS-098](P1-trust-release/CCS-098-p1-switch-provider-off-main.md) | bug | p3 | S | [providers] Run the switch_provider command off the main thread |  | Done |
| [CCS-099](P1-trust-release/CCS-099-p1-switch-lock-all-apps.md) | bug | p3 | S | [providers] Take the per-app switch lock for every app type | CCS-098 | Done |
| [CCS-100](P1-trust-release/CCS-100-p1-subscription-keychain-blocking.md) | bug | p2 | S | [usage] Run macOS keychain reads off the async workers, with a timeout |  | Partial |
| [CCS-101](P1-trust-release/CCS-101-p1-skill-zip-extraction.md) | security | p3 | S | [skills] Use enclosed_name and size limits when extracting skill repo archives |  | Done |
| [CCS-102](P1-trust-release/CCS-102-p1-gemini-env-writer.md) | bug | p3 | S | [gemini] Validate .env keys and values, strip quotes when parsing, and keep unknown lines | CCS-012 | Done |
| [CCS-103](P1-trust-release/CCS-103-p1-claude-json-optimistic.md) | bug | p3 | S | [claude] Use optimistic concurrency for read-modify-write of ~/.claude.json | CCS-079 | Done |
| [CCS-104](P1-trust-release/CCS-104-p1-powershell-quote.md) | bug | p3 | S | [terminal] Escape apostrophes in PowerShell launch paths | CCS-036 | Done |
| [CCS-105](P1-trust-release/CCS-105-p1-fe-jsoneditor-onchange.md) | bug | p2 | S | [editor] Stop JsonEditor capturing a stale onChange, which makes the MCP modal overwrite ID and name |  | Done |
| [CCS-106](P1-trust-release/CCS-106-p1-fe-managed-auth-timers.md) | bug | p2 | S | [auth-ui] Prevent leaked and overlapping managed-auth polling timers |  | Done |
| [CCS-107](P1-trust-release/CCS-107-p1-fe-edit-dialog-reset.md) | bug | p3 | S | [provider-form] Do not reset typed input when live settings finish loading in the edit dialog |  | Done |
| [CCS-108](P1-trust-release/CCS-108-p1-fe-mcp-stdio-validation.md) | bug | p3 | S | [mcp] Treat a missing type as stdio when validating MCP server specs, in the UI and the backend |  | Done |
| [CCS-109](P1-trust-release/CCS-109-p1-fe-skills-cache-dedupe.md) | bug | p3 | S | [skills] Dedupe the installed-skills cache by id and skip same-name folders in one ZIP |  | Done |
| [CCS-225](P1-trust-release/CCS-225-p1-settings-load-failure-notice.md) | bug | p2 | M | [settings-ui] Show a startup notice and confirm flow when settings.json failed to load |  | Open |
| [CCS-226](P1-trust-release/CCS-226-p1-e2e-csp-icons-runtime.md) | security | p2 | M | [e2e] Check the tightened CSP and asset-protocol removal at runtime with a headless WebKitGTK spec |  | Open |
| [CCS-227](P1-trust-release/CCS-227-p1-proxy-require-ipv6-guard.md) | chore | p3 | S | [proxy-tests] Fail the [::1] proxy tests instead of skipping when CCS_REQUIRE_IPV6 is set |  | Open |
| [CCS-228](P1-trust-release/CCS-228-p1-config-atomic-write-wine.md) | chore | p3 | S | [config] Run the Windows rename_replace / atomic_write path under Wine as interim evidence |  | Open |

## P2: [P2 (v3.20): Typed vocabulary, app registry, IPC contract and generated bindings](P2-typed-vocabulary/CCS-003-epic-p2.md)

| ID | Kind | Pri | Effort | Title | Depends on | Status |
|---|---|---|---|---|---|---|
| [CCS-110](P2-typed-vocabulary/CCS-110-p2-proxy-format-types.md) | refactor | p2 | M | [proxy] Introduce WireFormat, ClaudeApiFormat, AppRef and BreakerKey types | CCS-009, CCS-014 | Done |
| [CCS-111](P2-typed-vocabulary/CCS-111-p2-proxy-dead-code-purge.md) | refactor | p3 | S | [proxy] Delete dead proxy abstractions and cut #[allow(dead_code)] from 69 to 15 or fewer | CCS-110 | Done |
| [CCS-112](P2-typed-vocabulary/CCS-112-p2-app-registry.md) | refactor | p1 | M | [apps] Add the AppDescriptor registry and replace hard-coded app lists | CCS-012 | Done |
| [CCS-113](P2-typed-vocabulary/CCS-113-p2-app-registry-ipc.md) | refactor | p2 | S | [apps] Expose get_app_registry, add the exhaustiveness test, and fix the prompt and skills-dir edge cases | CCS-112 | Done |
| [CCS-114](P2-typed-vocabulary/CCS-114-p2-ipc-error-type.md) | refactor | p2 | M | [ipc] Introduce IpcError with stable error codes |  | Done |
| [CCS-115](P2-typed-vocabulary/CCS-115-p2-ipc-error-frontend.md) | refactor | p2 | S | [errors] Switch frontend error handling on IpcError.code | CCS-114 | Done |
| [CCS-116](P2-typed-vocabulary/CCS-116-p2-bindings-spike.md) | chore | p1 | S | [ipc] Time-boxed tauri-specta spike on the usage domain, with a written decision | CCS-114 | Done |
| [CCS-117](P2-typed-vocabulary/CCS-117-p2-bindings-generate.md) | refactor | p1 | L | [ipc] Generate src/shared/bindings.ts with a CI dirty check and typed MSW handlers | CCS-116 | Done |
| [CCS-118](P2-typed-vocabulary/CCS-118-p2-ipc-naming-orphans.md) | chore | p3 | M | [ipc] Normalize command and argument casing, and wire up or delete orphan commands | CCS-117 | Done |
| [CCS-119](P2-typed-vocabulary/CCS-119-p2-fe-app-registry.md) | refactor | p2 | M | [apps-ui] Add the frontend app registry and replace VALID_APPS and the capability chains | CCS-113 | Partial |
| [CCS-120](P2-typed-vocabulary/CCS-120-p2-eslint-ratchets.md) | chore | p2 | S | [lint] Ban raw invoke outside the bindings layer and ratchet app-literal comparisons | CCS-117, CCS-119, CCS-017 | Done |
| [CCS-121](P2-typed-vocabulary/CCS-121-p2-deps-modernize.md) | chore | p3 | M | [deps] Replace serde_yaml, consolidate the JSON5 crates and drop once_cell | CCS-012 | Done |
| [CCS-122](P2-typed-vocabulary/CCS-122-p2-feature-actionable-errors.md) | feature | p3 | M | [errors] Add actions to error toasts based on IpcError codes | CCS-115 | Done |
| [CCS-229](P2-typed-vocabulary/CCS-229-p2-fe-app-registry-from-backend.md) | refactor | p2 | M | [frontend] Feed useAppRegistry() from the backend get_app_registry command |  | Open |

## P3: [P3 (v3.21): Proxy request path with endpoint table, RoutePlan, attempt pipeline, canonical usage and stream taps](P3-proxy-request-path/CCS-004-epic-p3.md)

| ID | Kind | Pri | Effort | Title | Depends on | Status |
|---|---|---|---|---|---|---|
| [CCS-123](P3-proxy-request-path/CCS-123-p3-endpoint-table.md) | refactor | p2 | M | [proxy] Build routes from a ClientEndpoint table with one handle_proxied and per-format error renderers | CCS-110 | Done |
| [CCS-124](P3-proxy-request-path/CCS-124-p3-runtime-config-snapshot.md) | refactor | p2 | M | [proxy] Cache runtime config in an ArcSwap snapshot and remove DB reads from the request path | CCS-123 | Done |
| [CCS-125](P3-proxy-request-path/CCS-125-p3-router-lock-db-read.md) | bug | p3 | S | [proxy] Read the breaker config before taking the breaker-map write lock | CCS-124 | Done |
| [CCS-126](P3-proxy-request-path/CCS-126-p3-route-plan.md) | refactor | p1 | M | [proxy] Resolve a RoutePlan once per attempt and carry it in ForwardResult | CCS-123 | Done |
| [CCS-127](P3-proxy-request-path/CCS-127-p3-attempt-recorder.md) | refactor | p2 | M | [proxy] Add AttemptRecorder to own status, breaker, health and try_switch for each attempt | CCS-126, CCS-051 | Done |
| [CCS-128](P3-proxy-request-path/CCS-128-p3-failover-flapping.md) | bug | p3 | S | [proxy] Count failovers only when a request actually failed over, and report recovery separately | CCS-127 | Done |
| [CCS-129](P3-proxy-request-path/CCS-129-p3-recovery-strategy.md) | refactor | p2 | M | [proxy] Replace nested forward() retries with a RecoveryStrategy list | CCS-127 | Done |
| [CCS-130](P3-proxy-request-path/CCS-130-p3-request-pipeline-stages.md) | refactor | p2 | L | [proxy] Split forward() into request_builder, HeaderPolicy, transport and validate, leaving forwarder.rs a facade | CCS-129 | Done |
| [CCS-131](P3-proxy-request-path/CCS-131-p3-header-hop-by-hop.md) | security | p3 | S | [proxy] Strip hop-by-hop, cookie and vendor-identity headers from forwarded requests |  | Done |
| [CCS-132](P3-proxy-request-path/CCS-132-p3-header-case-per-request.md) | bug | p3 | M | [proxy] Capture original header casing per request instead of one peek per connection | CCS-014 | Done |
| [CCS-133](P3-proxy-request-path/CCS-133-p3-breaker-sliding-window.md) | bug | p3 | S | [proxy] Compute the circuit-breaker error rate over a sliding window |  | Done |
| [CCS-134](P3-proxy-request-path/CCS-134-p3-usage-ir.md) | refactor | p1 | M | [usage] Add a canonical UsageIR with from/to converters for each format | CCS-009, CCS-092 | Done |
| [CCS-135](P3-proxy-request-path/CCS-135-p3-stop-reason-table.md) | refactor | p2 | S | [proxy] Add a canonical StopReason table and unify the Gemini mapping | CCS-134, CCS-065 | Done |
| [CCS-136](P3-proxy-request-path/CCS-136-p3-sse-decoder.md) | refactor | p2 | M | [proxy] Replace the three SSE sniffers with one SseDecoder | CCS-126 | Done |
| [CCS-137](P3-proxy-request-path/CCS-137-p3-stream-traits-taps.md) | refactor | p2 | L | [proxy] Wrap converters behind StreamDecoder/Encoder, and add UsageTap, TimingTap and aggregate | CCS-136, CCS-134 | Done |
| [CCS-138](P3-proxy-request-path/CCS-138-p3-bug-report-capture.md) | feature | p2 | M | [proxy] Capture and export a redacted bug report in the golden-fixture layout | CCS-137 | Done |
| [CCS-139](P3-proxy-request-path/CCS-139-p3-trace-inspector.md) | feature | p2 | M | [proxy] Add a per-request trace inspector | CCS-126, CCS-127 | Done |
| [CCS-140](P3-proxy-request-path/CCS-140-p3-usage-parity-badge.md) | feature | p3 | S | [usage] Show a parity badge when normalized usage disagrees with the raw upstream usage | CCS-134, CCS-137 | Done |
| [CCS-141](P3-proxy-request-path/CCS-141-p3-breaker-ui-probe.md) | feature | p3 | M | [proxy] Show live circuit-breaker state with a "probe now" action | CCS-111, CCS-051 | Done |
| [CCS-142](P3-proxy-request-path/CCS-142-p3-retry-after-cooldown.md) | feature | p3 | S | [proxy] Honour Retry-After on 429 and 529 with a per-provider cooldown | CCS-127 | Done |
| [CCS-143](P3-proxy-request-path/CCS-143-p3-session-sticky-routing.md) | feature | p3 | S | [proxy] Optionally keep each session_id on one provider so prompt caches survive | CCS-128 | Done |
| [CCS-144](P3-proxy-request-path/CCS-144-p3-deps-dedupe-http.md) | chore | p3 | M | [deps] Dedupe tower, tower-http, webpki-roots and reqwest versions | CCS-041, CCS-014 | Done |
| [CCS-230](P3-proxy-request-path/CCS-230-p3-proxy-forwarder-under-600.md) | refactor | p3 | S | [proxy] Shrink proxy/forwarder.rs to 600 lines or fewer |  | Open |
| [CCS-231](P3-proxy-request-path/CCS-231-p3-proxy-p50-remeasure.md) | chore | p3 | M | [proxy-perf] Re-measure p50 proxy overhead against the P0 baseline and trim hot-path allocations if it regressed | CCS-230 | Open |

## P4: [P4 (v3.22): Per-app adapters, takeover engine, persistence and migration framework](P4-app-adapters-persistence/CCS-005-epic-p4.md)

| ID | Kind | Pri | Effort | Title | Depends on | Status |
|---|---|---|---|---|---|---|
| [CCS-145](P4-app-adapters-persistence/CCS-145-p4-migration-framework.md) | refactor | p1 | M | [db] Add a versioned migration framework with a frozen baseline and audit table | CCS-011, CCS-091 | Done |
| [CCS-146](P4-app-adapters-persistence/CCS-146-p4-adapter-trait-gemini-grok.md) | refactor | p1 | L | [apps] Add the AppAdapter trait and migrate Gemini and Grok | CCS-112, CCS-082 | Done |
| [CCS-147](P4-app-adapters-persistence/CCS-147-p4-adapter-claude.md) | refactor | p2 | L | [apps] Migrate Claude and Claude Desktop to AppAdapter | CCS-146 | Done |
| [CCS-148](P4-app-adapters-persistence/CCS-148-p4-adapter-codex.md) | refactor | p2 | L | [apps] Migrate Codex to AppAdapter, keeping the history-migration ids and markers stable | CCS-146 | Done |
| [CCS-149](P4-app-adapters-persistence/CCS-149-p4-adapter-additive.md) | refactor | p2 | L | [apps] Migrate OpenCode, OpenClaw and Hermes to AppAdapter and remove the remaining fan-out | CCS-147, CCS-148 | Partial |
| [CCS-150](P4-app-adapters-persistence/CCS-150-p4-adapter-conformance.md) | chore | p1 | M | [apps] Add an adapter conformance suite that loops over APPS, plus the dummy 9th-app test | CCS-149 | Done |
| [CCS-151](P4-app-adapters-persistence/CCS-151-p4-mcp-projector.md) | refactor | p2 | M | [mcp] Add McpProjector and generic MCP import loops | CCS-146, CCS-011 | Done |
| [CCS-152](P4-app-adapters-persistence/CCS-152-p4-takeover-crash-e2e.md) | chore | p1 | M | [takeover] Add crash/restore E2E for each proxy-capable app and a legacy-path runtime flag | CCS-014, CCS-012 | Partial |
| [CCS-153](P4-app-adapters-persistence/CCS-153-p4-services-proxy-split.md) | refactor | p2 | M | [proxy] Split services/proxy.rs (7.2k lines) into lifecycle, takeover, backup, hot_switch and config modules | CCS-152, CCS-054 | Done |
| [CCS-154](P4-app-adapters-persistence/CCS-154-p4-takeover-engine.md) | refactor | p2 | L | [proxy] Unify takeover behind takeover(app, Strict\|BestEffort) with TakeoverError and a registry-driven status | CCS-153, CCS-149 | Done |
| [CCS-155](P4-app-adapters-persistence/CCS-155-p4-per-app-settings-state.md) | refactor | p2 | M | [settings] Use keyed per-app maps with old-key dual-write, and migrate profiles and localStorage | CCS-112, CCS-085 | Done |
| [CCS-156](P4-app-adapters-persistence/CCS-156-p4-join-tables.md) | refactor | p2 | L | [db] Replace per-app MCP and skill columns with join tables, dual-writing the old columns for one release | CCS-145 | Done |
| [CCS-157](P4-app-adapters-persistence/CCS-157-p4-pricing-engine.md) | refactor | p2 | L | [pricing] Add the pricing catalog, resolver and a single cost engine, and delete the hand-written backfill | CCS-145, CCS-092 | Done |
| [CCS-158](P4-app-adapters-persistence/CCS-158-p4-pricing-provenance.md) | refactor | p2 | M | [pricing] Add pricing provenance and tombstones, classifying existing edited rows as user-owned | CCS-157, CCS-094 | Done |
| [CCS-159](P4-app-adapters-persistence/CCS-159-p4-usage-repo.md) | refactor | p2 | L | [db] Add usage and session-cursor repositories with a UsageQuery builder and transactional ingest | CCS-145, CCS-097 | Done |
| [CCS-160](P4-app-adapters-persistence/CCS-160-p4-superseded-by-backfill.md) | refactor | p2 | M | [db] Replace the correlated EXISTS with a superseded_by column and a chunked, resumable backfill | CCS-159 | Done |
| [CCS-161](P4-app-adapters-persistence/CCS-161-p4-rollup-retention.md) | refactor | p3 | M | [db] Store rollup cost as integer micro-USD and add RetentionPolicy with Maintenance::run | CCS-159 | Done |
| [CCS-162](P4-app-adapters-persistence/CCS-162-p4-json-key-order.md) | bug | p3 | S | [config] Keep user key order when rewriting user-owned JSON configs | CCS-149 | Done |
| [CCS-163](P4-app-adapters-persistence/CCS-163-p4-takeover-additive-apps.md) | feature | p2 | M | [proxy] Support proxy takeover, failover and usage for OpenCode, OpenClaw and Hermes | CCS-154 | Done |
| [CCS-164](P4-app-adapters-persistence/CCS-164-p4-openclaw-mcp.md) | feature | p3 | S | [mcp] Add OpenClaw MCP support (Issue #4834) | CCS-151, CCS-149 | Done |
| [CCS-165](P4-app-adapters-persistence/CCS-165-p4-capability-matrix.md) | feature | p3 | S | [settings] Add a capability matrix page | CCS-119, CCS-150 | Done |
| [CCS-166](P4-app-adapters-persistence/CCS-166-p4-drift-detection.md) | feature | p2 | M | [apps] Detect live-config drift and external edits, and offer reconcile | CCS-154 | Done |
| [CCS-167](P4-app-adapters-persistence/CCS-167-p4-budget-alerts.md) | feature | p3 | S | [usage] Add budget alerts on usage-log-recorded | CCS-159, CCS-095 | Done |
| [CCS-168](P4-app-adapters-persistence/CCS-168-p4-spend-cap-enforcement.md) | feature | p2 | M | [proxy] Enforce per-provider spend and token caps in provider selection | CCS-159, CCS-124 | Done |
| [CCS-169](P4-app-adapters-persistence/CCS-169-p4-retention-panel.md) | feature | p3 | S | [settings] Add a retention and DB-size panel | CCS-161 | Done |
| [CCS-170](P4-app-adapters-persistence/CCS-170-p4-live-config-journal.md) | feature | p3 | M | [apps] Keep a live-config journal with one-click undo of the last switch | CCS-149 | Done |
| [CCS-171](P4-app-adapters-persistence/CCS-171-p4-usage-rescan.md) | feature | p3 | M | [usage] Add a usage re-scan action and a per-source data-quality report | CCS-159 | Done |
| [CCS-172](P4-app-adapters-persistence/CCS-172-p4-keychain-secrets.md) | feature | p2 | L | [security] Store secrets in the OS keychain or credential manager | CCS-083, CCS-155 | Partial |
| [CCS-173](P4-app-adapters-persistence/CCS-173-p4-sync-snapshot-auth.md) | feature | p2 | M | [sync] Authenticate WebDAV/S3 snapshots with an HMAC and refuse unsigned remote imports | CCS-086, CCS-172 | Partial |
| [CCS-212](P4-app-adapters-persistence/CCS-212-p4-hot-switch-rollback-snapshot.md) | bug | p3 | S | [takeover] Keep the raw byte snapshot when a failed hot switch rolls back the live backup | CCS-152 | Done |
| [CCS-232](P4-app-adapters-persistence/CCS-232-p4-apps-budget-ratchet.md) | chore | p2 | S | [apps-tests] Make the match-app counter skip test-only modules and ratchet both app budgets to today's counts |  | Open |
| [CCS-233](P4-app-adapters-persistence/CCS-233-p4-apps-ninth-app-six-files.md) | refactor | p2 | M | [apps] Make a 9th app touch 6 or fewer backend files | CCS-232 | Done |
| [CCS-234](P4-app-adapters-persistence/CCS-234-p4-takeover-dispatch-via-adapter.md) | refactor | p2 | L | [takeover] Route takeover per-app dispatch through ProxyTakeover instead of AppType matches | CCS-232 | Open |
| [CCS-235](P4-app-adapters-persistence/CCS-235-p4-apps-config-profile-keyed-access.md) | refactor | p3 | M | [apps] Replace per-app field matches in app_config.rs and services/profile.rs with keyed access | CCS-232 | Open |
| [CCS-236](P4-app-adapters-persistence/CCS-236-p4-apps-match-sites-under-15.md) | refactor | p3 | M | [apps] Move the remaining stream-check, terminal and proxy-provider dispatch into adapters and set the match-app budget to 15 or fewer | CCS-233, CCS-234, CCS-235 | Open |
| [CCS-237](P4-app-adapters-persistence/CCS-237-p4-sync-key-field-api.md) | feature | p2 | M | [sync] Add the sync-key field to sync settings and expose signature/allowUnsigned in the API layer |  | Open |
| [CCS-238](P4-app-adapters-persistence/CCS-238-p4-sync-unsigned-migration-prompt.md) | feature | p2 | M | [sync] Prompt before applying unsigned snapshots and block tampered ones, with i18n for sync.* errors | CCS-237 | Open |
| [CCS-239](P4-app-adapters-persistence/CCS-239-p4-sync-manifest-rollback-protection.md) | security | p3 | M | [sync] Reject signed sync manifests older than the last applied one |  | Open |
| [CCS-240](P4-app-adapters-persistence/CCS-240-p4-secrets-linux-secret-service-check.md) | chore | p3 | M | [secrets] Exercise KeyringSecretStore against a headless gnome-keyring and the no-D-Bus fallback |  | Open |

## P5: [P5 (v4.0): Canonical protocol IR and codecs, plus command-layer cleanup](P5-protocol-ir-commands/CCS-006-epic-p5.md)

| ID | Kind | Pri | Effort | Title | Depends on | Status |
|---|---|---|---|---|---|---|
| [CCS-174](P5-protocol-ir-commands/CCS-174-p5-ir-codec-core.md) | refactor | p2 | M | [proxy] Define the canonical protocol IR and codec traits | CCS-137, CCS-135 | Done |
| [CCS-175](P5-protocol-ir-commands/CCS-175-p5-session-side-store.md) | refactor | p2 | M | [proxy] Move the Gemini shadow, Codex chat history and reasoning bridge behind SessionSideStore | CCS-174 | Done |
| [CCS-176](P5-protocol-ir-commands/CCS-176-p5-codec-openai-chat.md) | refactor | p2 | L | [proxy] Migrate the Anthropic<-OpenAI Chat bridge to codecs | CCS-175 | Done |
| [CCS-177](P5-protocol-ir-commands/CCS-177-p5-codec-gemini.md) | refactor | p2 | L | [proxy] Migrate the Gemini bridge to codecs | CCS-176 | Done |
| [CCS-178](P5-protocol-ir-commands/CCS-178-p5-codec-responses.md) | refactor | p2 | XL | [proxy] Migrate the Responses bridges to codecs and remove the legacy engine | CCS-177 | Partial |
| [CCS-179](P5-protocol-ir-commands/CCS-179-p5-misc-split-terminal.md) | refactor | p2 | L | [commands] Split misc.rs into cli_tools services and a unified platform terminal stack | CCS-104, CCS-036 | Partial |
| [CCS-180](P5-protocol-ir-commands/CCS-180-p5-command-domains-permissions.md) | security | p2 | M | [ipc] Register commands per domain with collect_commands! and scope them with AppManifest permissions | CCS-179, CCS-117, CCS-075 | Done |
| [CCS-181](P5-protocol-ir-commands/CCS-181-p5-thin-commands.md) | refactor | p2 | L | [commands] Make commands thin adapters over a Ctx/EventSink seam and reach 100% IpcError | CCS-180, CCS-114 | Done |
| [CCS-182](P5-protocol-ir-commands/CCS-182-p5-app-shell-bootstrap.md) | refactor | p2 | M | [shell] Add a StartupStep pipeline with a startup diagnostics report and a single run-event router | CCS-181 | Done |
| [CCS-183](P5-protocol-ir-commands/CCS-183-p5-tray-split.md) | refactor | p3 | M | [tray] Split the tray into model, menu and events driven by an event bus | CCS-182 | Done |
| [CCS-184](P5-protocol-ir-commands/CCS-184-p5-session-resume-cross-platform.md) | feature | p3 | M | [sessions] Support session resume on Windows and Linux | CCS-179, CCS-074 | Partial |
| [CCS-185](P5-protocol-ir-commands/CCS-185-p5-midstream-failover.md) | feature | p2 | M | [proxy] Fail over on errors that arrive before any content is streamed | CCS-174, CCS-127 | Done |
| [CCS-186](P5-protocol-ir-commands/CCS-186-p5-usage-parity-ci.md) | chore | p3 | S | [usage] Run a usage parity checker over the fixture corpus in CI | CCS-140, CCS-178 | Done |
| [CCS-187](P5-protocol-ir-commands/CCS-187-p5-gemini-cli-anthropic.md) | feature | p3 | M | [proxy] Add the first new client/upstream combination: Gemini CLI to an Anthropic upstream | CCS-177 | Done |
| [CCS-188](P5-protocol-ir-commands/CCS-188-p5-tool-lifecycle-streaming.md) | feature | p3 | M | [cli-tools] Stream installer output and support cancel and timeout for tool lifecycle actions | CCS-179 | Done |
| [CCS-241](P5-protocol-ir-commands/CCS-241-p5-commands-settings-test-split.md) | refactor | p3 | S | [commands] Move the commands/settings.rs test module into its own file |  | Open |
| [CCS-242](P5-protocol-ir-commands/CCS-242-p5-coverage-min-threshold.md) | chore | p3 | S | [coverage] Add a ratcheting --min threshold to coverage-summary.mjs and gate coverage-rust on it | CCS-222 | Open |
| [CCS-243](P5-protocol-ir-commands/CCS-243-p5-usage-token-name-check.md) | chore | p3 | S | [usage] Add a check script that keeps cache-token field names inside codec/ and usage/ |  | Open |
| [CCS-244](P5-protocol-ir-commands/CCS-244-p5-usage-token-names-out-of-taps.md) | refactor | p3 | M | [usage] Move cache-token field parsing out of stream taps, session_usage and the provider testkit | CCS-243 | Open |
| [CCS-245](P5-protocol-ir-commands/CCS-245-p5-proxy-repoint-legacy-reexports.md) | refactor | p3 | S | [proxy] Repoint non-legacy callers from legacy transform re-exports to codec::* |  | Open |
| [CCS-246](P5-protocol-ir-commands/CCS-246-p5-codec-engine-ir-default.md) | chore | p2 | S | [settings] Prepare the default flip to codecEngine = ir (merge needs a maintainer release decision) |  | Open |
| [CCS-247](P5-protocol-ir-commands/CCS-247-p5-terminal-applescript-builders-neutral.md) | refactor | p3 | S | [terminal] Move the macOS AppleScript builders into an OS-neutral module so their tests run on Linux |  | Open |
| [CCS-248](P5-protocol-ir-commands/CCS-248-p5-e2e-session-resume-linux.md) | chore | p3 | M | [e2e] Automate the Linux session-resume check under xvfb with a fake claude |  | Open |
| [CCS-249](P5-protocol-ir-commands/CCS-249-p5-terminal-resume-bat-wine.md) | chore | p3 | S | [terminal] Run the generated Windows resume .bat under Wine cmd |  | Open |

## F: [Track F: Frontend decomposition (runs in parallel from P2 bindings, lands by v4.0)](F-frontend/CCS-007-epic-f.md)

| ID | Kind | Pri | Effort | Title | Depends on | Status |
|---|---|---|---|---|---|---|
| [CCS-189](F-frontend/CCS-189-f-settingsconfig-goldens.md) | chore | p1 | S | [provider-form] Add per-app, per-preset settingsConfig golden snapshots | CCS-039 | Done |
| [CCS-190](F-frontend/CCS-190-f-providerform-shell.md) | refactor | p2 | L | [provider-form] Turn ProviderForm into a shell with a base schema, a discriminated union and an inline error summary | CCS-189, CCS-119 | Done |
| [CCS-191](F-frontend/CCS-191-f-providerform-core-apps.md) | refactor | p2 | L | [provider-form] Extract the Claude, Claude Desktop, Grok, Codex and Gemini form modules | CCS-190 | Done |
| [CCS-192](F-frontend/CCS-192-f-providerform-additive-apps.md) | refactor | p2 | L | [provider-form] Extract the OpenCode, OpenClaw and Hermes form modules, with component tests | CCS-190 | Done |
| [CCS-193](F-frontend/CCS-193-f-i18n-validation-messages.md) | bug | p3 | M | [i18n] Localize zod and editor validation messages | CCS-190, CCS-020 | Partial |
| [CCS-194](F-frontend/CCS-194-f-query-keys.md) | refactor | p2 | M | [query] Add query-key factories, invalidateProviderGraph and per-query staleTime | CCS-119 | Done |
| [CCS-195](F-frontend/CCS-195-f-mutation-appid-variables.md) | bug | p3 | S | [query] Bind provider mutation invalidation to the variables' appId, not activeApp | CCS-194 | Done |
| [CCS-196](F-frontend/CCS-196-f-mutation-live-ids.md) | bug | p3 | S | [query] Invalidate the OpenCode and OpenClaw live-provider-id caches on add, update and delete | CCS-194 | Done |
| [CCS-197](F-frontend/CCS-197-f-app-shell-split.md) | refactor | p2 | M | [shell] Split App.tsx into AppHeader, a ViewTitle map and confirm orchestration | CCS-119 | Done |
| [CCS-198](F-frontend/CCS-198-f-lazy-views-locales.md) | refactor | p3 | M | [perf] Lazy-load heavy views, per-app presets and locales | CCS-197 | Done |
| [CCS-199](F-frontend/CCS-199-f-typed-i18n.md) | refactor | p3 | M | [i18n] Type the i18n keys and remove the 691 inline defaultValue fallbacks | CCS-020, CCS-198 | Done |
| [CCS-200](F-frontend/CCS-200-f-radix-dialog-component-tests.md) | refactor | p3 | M | [ui] Move custom overlays to Radix Dialog and add component tests for the large panels | CCS-191 | Done |
| [CCS-201](F-frontend/CCS-201-f-tauri-driver-e2e.md) | chore | p2 | M | [e2e] Add a tauri-driver and WebdriverIO smoke on Linux that gates release | CCS-024, CCS-023 | Partial |
| [CCS-202](F-frontend/CCS-202-f-resize-listener-leak.md) | bug | p3 | S | [shell] Unregister the onResized listener when App unmounts before it resolves |  | Done |
| [CCS-203](F-frontend/CCS-203-f-safe-storage.md) | bug | p3 | S | [shell] Wrap all localStorage access in safe helpers |  | Done |
| [CCS-204](F-frontend/CCS-204-f-a11y-icon-buttons.md) | bug | p3 | S | [a11y] Give icon-only buttons accessible names |  | Done |
| [CCS-205](F-frontend/CCS-205-f-live-diff-preview.md) | feature | p3 | M | [provider-form] Preview the diff of the target live config before saving | CCS-190, CCS-149 | Done |
| [CCS-206](F-frontend/CCS-206-f-unsaved-changes-guard.md) | feature | p3 | S | [provider-form] Guard against closing the Add/Edit panel with unsaved changes | CCS-190, CCS-200 | Done |
| [CCS-207](F-frontend/CCS-207-f-switching-state.md) | feature | p3 | S | [providers] Show a switching state and disable the other Switch buttons while a switch is pending | CCS-195 | Done |
| [CCS-208](F-frontend/CCS-208-f-command-palette.md) | feature | p3 | S | [shell] Add a Cmd/Ctrl-K command palette to search and switch providers across apps | CCS-194, CCS-207 | Done |
| [CCS-209](F-frontend/CCS-209-f-unpriced-requests-export.md) | feature | p3 | S | [usage] Filter and export unpriced requests, with a shortcut to add model pricing | CCS-159, CCS-021 | Done |
| [CCS-210](F-frontend/CCS-210-f-markdowneditor-stale-onchange.md) | bug | p3 | S | [editor] Stop MarkdownEditor capturing a stale onChange |  | Done |
| [CCS-211](F-frontend/CCS-211-f-lint-green.md) | chore | p1 | S | [lint] Make pnpm lint green again after the P1/P2 merges |  | Done |
| [CCS-250](F-frontend/CCS-250-f-app-shell-under-300.md) | refactor | p2 | M | [app-shell] Shrink src/App.tsx to 300 lines or fewer by extracting the command palette wiring |  | Open |
| [CCS-251](F-frontend/CCS-251-f-lint-app-literals-batch1.md) | refactor | p2 | M | [lint] Replace app-literal comparisons in the five largest offenders with registry capabilities |  | Open |
| [CCS-252](F-frontend/CCS-252-f-lint-app-literals-batch2.md) | refactor | p2 | M | [lint] Bring the app-literal suppression count to 40 or fewer | CCS-251 | Open |
| [CCS-253](F-frontend/CCS-253-f-i18n-usage-cjk-matchers.md) | bug | p3 | S | [i18n] Remove the dead Chinese usage-script matchers from queries.ts and close CCS-193 |  | Open |
