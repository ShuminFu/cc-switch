# Acceptance playbook

This page holds the process rules from the Phase 0 safety net
([next-phase-plan.md](../roadmap/next-phase-plan.md), "CI gates" and "Process
rules"). It also gives two step-by-step recipes: **fix a proxy bug** and
**add an app**. A PR is ready for review when it meets the checklist of the
recipe it follows.

## The rules

1. **Every proxy bug fix adds a fixture first.** The fixture reproduces the
   bug and lands in the same PR as the fix. Write it before the fix, so the
   fixture is seen failing on the old code.
2. **A change under `src-tauri/src/proxy/**` needs a fixture change.** The PR
   must also change something under `src-tauri/tests/fixtures/proxy/`. If no
   fixture can capture the change (a log message, a comment, a pure rename, a
   dependency bump), a maintainer adds the `no-fixture-needed` label. The
   [`proxy-fixture.yml`](../../.github/workflows/proxy-fixture.yml) workflow
   enforces this (see [The fixture check](#the-fixture-check)).
3. **A structural PR keeps the old path callable until parity is shown.** A
   PR that moves or rewrites a module adds the new path next to the old one,
   shows that both give the same output on the fixtures, and switches callers
   over. The old path is deleted in a later PR. Do not use a long-lived
   branch: each step lands on `main`.
4. **Fix commits per release are tracked as a KPI** (see
   [Fix-commit KPI](#fix-commit-kpi)).

## Fix a proxy bug

1. **Find the bridge.** Golden cases live in
   `src-tauri/tests/fixtures/proxy/<bridge>/<case>/`. The bridges are
   `anthropic__openai_chat`, `anthropic__gemini`, `anthropic__responses`,
   `responses__chat` and `responses__anthropic`. The layout of a case and the
   `meta.toml` keys are documented at the top of
   `src-tauri/src/proxy/providers/testkit.rs`.
2. **Add a case that shows the bug.** Create a new case directory with:
   - `meta.toml`: `intent` (one sentence: what the case pins) and
     `source_commit` (the commit the case was written against; the fix
     commit's short SHA once it exists);
   - the input: `client_request.json` for a request conversion, plus
     `upstream_response.sse` (stream) or `upstream_response.json`
     (non-stream). Use a real, redacted capture when you have one. Keep raw
     bytes (CRLF, split UTF-8) exactly as they arrived; `.gitattributes`
     marks `*.sse` as `-text`, so git never rewrites their line endings.
3. **Record the current (wrong) behaviour.**
   `INSTA_UPDATE=always cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`
   writes the `.snap` files. Check that they show the bug. Commit this as the
   first commit of the PR (`test(proxy): ...`), so reviewers can see the
   fixture on the old code.
4. **Fix the bug** in `src-tauri/src/proxy/**`.
5. **Re-run the golden test and review the snapshot diff**
   (`cargo insta review`, or `INSTA_UPDATE=always` again followed by
   `git diff`). Only the new case, plus any cases that share the bug, may
   change. Each changed snapshot must be explained in the PR. A
   stream/non-stream drift that is kept on purpose is recorded in
   `[parity] expected_diff` instead of being hidden.
6. **Run the checks.** Run `cargo test --manifest-path src-tauri/Cargo.toml`
   (the golden test, chunk-split fuzzing and the inline proxy tests) and
   `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`.
   If token counts or costs change, also update the cost golden table in
   `src-tauri/src/proxy/usage/cost_golden.rs`.
7. **Commit the fix as `fix(proxy): ...`.** The KPI script counts these.

Checklist:

- [ ] A new or updated case under `src-tauri/tests/fixtures/proxy/` shows the
      bug, and its `meta.toml` has an `intent`
- [ ] The fixture commit comes before the fix commit
- [ ] Every snapshot change is explained in the PR description
- [ ] `proxy_golden` and the full `cargo test` suite are green

## Add an app

An app is one value of `AppType` (`src-tauri/src/app_config.rs`) and of
`AppId` (`src/lib/api/types.ts`). Adding Grok Build touched about 112 files,
so treat the list below as a minimum and use
`git grep -n "AppType::Hermes"` and `git grep -n "\"hermes\""` to find every
`match` site and string switch. The compiler finds the exhaustive `match`
sites. It does not find `_ =>` arms or string comparisons, so read those one
by one.

Backend (`src-tauri/src/`):

1. `app_config.rs`: add the `AppType` variant, its `as_str`/`FromStr`
   spelling and any serde aliases, plus its fields in the per-app config
   structs. Extend `tests/app_type_parse.rs`.
2. `paths.rs` and `settings.rs`: add the config directory, the directory
   override and the visibility setting.
3. The live config writer: add a `<app>_config.rs` module, then wire it into
   `services/provider/live.rs` and `services/config.rs`. Add a round-trip test
   whose input includes comments, unknown keys, CRLF and a BOM (see
   `tests/hermes_roundtrip.rs`).
4. Providers, MCP, skills and prompts: update `provider.rs`,
   `services/provider/mod.rs`, `mcp/<app>.rs` and `mcp/mod.rs`,
   `services/mcp.rs`, `services/skill.rs` and `prompt_files.rs`. If the app
   cannot support a feature, return an explicit "unsupported" error instead
   of silently doing nothing.
5. Database: update `database/schema.rs` (the `enabled_<app>` columns), the
   DAOs under `database/dao/` and `services/sql_helpers.rs`. Any schema change
   is a new migration with a bumped `SCHEMA_VERSION`, and it ships with a
   fixture for the migration harness.
6. Proxy (only if the app goes through the proxy): update
   `proxy/providers/mod.rs`, `proxy/handlers.rs`, `proxy/server.rs`,
   `proxy/usage/{calculator,logger}.rs`, the cost golden table, and
   `services/proxy.rs` for takeover. This is a `proxy/**` change, so add at
   least one golden case for each bridge the app uses.
7. Deep links (`deeplink/{parser,provider,mcp}.rs` and `deeplink/tests.rs`),
   the session manager (`session_manager/providers/<app>.rs`), `tray.rs`, and
   the command layer (`commands/config.rs`, `commands/proxy.rs`), and the
   CLI tool tables in `services/cli_tools/` (if the app ships a CLI).
8. Integration tests: add the app to `tests/support.rs` and to the provider,
   MCP and import/export suites.

Frontend (`src/`):

1. `lib/api/types.ts` (`AppId`), `types.ts`, `types/proxy.ts`,
   `types/usage.ts` and `lib/schemas/settings.ts`.
2. `config/appConfig.tsx` (name, icon, order), `components/AppSwitcher.tsx`,
   `App.tsx`, and settings (`AppVisibilitySettings`, `DirectorySettings`,
   `hooks/useDirectorySettings.ts`, `hooks/useSettings*.ts`).
3. Provider forms and presets: `components/providers/**`,
   `config/<app>ProviderPresets.ts`, and a `utils/<app>Config.ts` helper with
   a unit test.
4. MCP, skills, prompts, sessions, proxy and usage panels that switch on the
   app id.
5. i18n: add the same keys to every file in `src/i18n/locales/`.
6. Tests: add the app to the MSW handlers in `tests/msw/` and extend the
   component and hook tests that enumerate apps.

Docs: add the app to the README files and the user manual.

Checklist:

- [ ] `git grep` for an existing app's variant and id shows every site
      updated, or explicitly left unsupported
- [ ] Config writer round-trip test, `app_type_parse` and deep-link tests
      cover the new app
- [ ] A golden proxy case for each bridge the app uses (if it uses the proxy)
- [ ] Every locale has the new keys; `pnpm typecheck`, `pnpm test:unit` and
      `cargo test` are green

## The fixture check

`.github/workflows/proxy-fixture.yml` runs `scripts/check-proxy-fixture.mjs`
on every PR to `main`. It also re-runs when labels are added or removed. The
script diffs the PR from its merge base (`base...head`) and fails when files
under `src-tauri/src/proxy/` changed but nothing under
`src-tauri/tests/fixtures/proxy/` did, unless the PR has the
`no-fixture-needed` label. The labeler adds a `fixtures` label to PRs that
change the corpus. `no-fixture-needed` is set by hand only. It must not be
listed in `.github/labeler.yml`, because `sync-labels` would remove it.

To run the check locally against `main`:

```bash
node scripts/check-proxy-fixture.mjs --base origin/main --head HEAD
node scripts/check-proxy-fixture.mjs --base origin/main --label no-fixture-needed
```

`tests/scripts/checkProxyFixture.test.ts` builds a throwaway repository with
a test PR that touches `proxy/`. It checks that the PR fails without a fixture
or label, and passes with either one.

The check only blocks merging once **Proxy fixture required** is a required
status check in the branch protection rule for `main` (repository settings).

## Fix-commit KPI

`scripts/kpi-fix-commits.sh` counts commits per release window
(`<previous tag>..<tag>`, merges excluded):

- **Fix commits:** the subject starts with `fix:`, `fix(scope):`, `fix!:`,
  `revert...:` or `Revert "`.
- **Proxy fix commits:** fix commits that touch `src-tauri/src/proxy/`.

It needs the release tags and full history:

```bash
git fetch --tags --unshallow   # once, in a shallow clone
scripts/kpi-fix-commits.sh     # last 3 releases reachable from HEAD
scripts/kpi-fix-commits.sh --ref v3.17.0 5
```

When cutting a release, run it and paste the row for the new release into the
release notes (see [RELEASING.md](./RELEASING.md)). The Phase 5 exit
criterion is that proxy fix commits per release fall below the baseline
below.

### Baseline (recorded 2026-10-28)

This branch forked from upstream at `6d2ee24`, which sits between v3.16.5 and
v3.17.0. The baseline is the last 3 releases before that point. It was
generated with `scripts/kpi-fix-commits.sh -C <full clone of
farion1231/cc-switch> --ref 6d2ee24`, because this repository's clone has no
tags:

| Release | Range | Commits | Fix commits | Fix % | Proxy fix commits |
|---------|-------|--------:|------------:|------:|------------------:|
| v3.16.3 | `v3.16.2..v3.16.3` | 60 | 25 | 41% | 5 |
| v3.16.4 | `v3.16.3..v3.16.4` | 57 | 12 | 21% | 4 |
| v3.16.5 | `v3.16.4..v3.16.5` | 39 | 12 | 30% | 0 |
| unreleased | `v3.16.5..6d2ee24` | 32 | 16 | 50% | 1 |

**Baseline: 49 fix commits out of 156 (31%) over 3 releases, and 9 proxy fix
commits, which is 3.0 per release.** For context, the v3.17.0 window
(`v3.16.5..v3.17.0`) had 71 commits, 34 fix commits (47%) and 11 proxy fix
commits.
