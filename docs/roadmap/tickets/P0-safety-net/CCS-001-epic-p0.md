# CCS-001: [epic] P0 (v3.18): Safety net, real CI gates and zero-risk hotfixes

**Status:** Open · **Kind:** epic · **Labels:** `epic`, `phase:P0`, `p0-critical`, `area:tests`

> **Ticket key:** `epic-p0` · **Phase:** P0 · **Priority:** p0-critical · **Effort:** XL
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Pin current behaviour in the areas that regress most (protocol bridges, cost accounting, migrations, live-config writers, proxy lifecycle) and make CI enforce what CONTRIBUTING.md claims. Ship a handful of hotfixes that need no harness.

## Exit criteria
- [ ] At least 60 golden bridge fixtures across all 5 bridges (stream and non-stream), passing under every-offset, random, UTF-8 and CRLF chunk splitting; the ~1046 existing inline proxy tests still pass
- [ ] Cost golden table covers 8 apps x 3 semantics; the only failures are the known grokbuild backfill rows, marked #[ignore]
- [ ] Migration harness passes from at least 10 start versions plus the legacy config.json path (migrate_from_json)
- [ ] Config-writer snapshots exist for all 11 live writers plus the Copilot/Codex OAuth token stores and the Codex state DB; untouched bytes stay identical
- [ ] test_mutex is removed, integration tests run in parallel, and proxy_e2e has at least 8 scenarios, green on 3 OSes
- [ ] CI enforces clippy --all-targets, ESLint (baseline), Prettier on tests/, cargo-deny and i18n parity; a tag with no green CI cannot publish
- [ ] Release pipeline: SHA-pinned actions, secrets passed via env:, missing updater .sig is fatal
- [ ] No log line or webview event contains an unredacted ccswitch:// URL

## Child tickets
p0-golden-testkit-chat-gemini, p0-golden-responses-fixtures, p0-cost-golden-table, p0-migration-harness, p0-config-writer-snapshots, p0-testenv-paths, p0-proxy-harness-e2e, p0-untested-module-tests, p0-ci-clippy-all-targets, p0-ci-eslint-prettier, p0-ci-cargo-deny, p0-ci-coverage-report, p0-ci-i18n-parity, p0-i18n-missing-keys, p0-i18n-zhtw-keys, p0-ci-release-gated, p0-ci-build-smoke, p0-ci-toolchain-msrv, p0-ci-node-version, p0-ci-permissions, p0-ci-pin-actions, p0-ci-release-secrets-env, p0-ci-updater-sig-fatal, p0-ci-release-concurrency, p0-ci-prerelease-promotion, p0-ci-release-hygiene, p0-dev-acceptance-process, p0-hotfix-deeplink-redaction, p0-hotfix-terminal-tempfiles, p0-hotfix-sync-skip-session-log, p0-hotfix-session-sync-blocking, p0-hotfix-providerform-parse, p0-failure-threshold-const, p0-unused-tower-http, p0-icon-scripts-cjs, p0-ci-stale-workflow, p0-flatpak-manifest, p0-docs-changelog-unreleased, p0-docs-readme-release-sync, p0-docs-package-description, p0-docs-root-clutter
