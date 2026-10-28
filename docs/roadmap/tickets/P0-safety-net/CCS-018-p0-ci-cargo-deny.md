# CCS-018: [ci] Gate on dependency advisories with cargo-deny and pnpm audit

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci`

> **Ticket key:** `p0-ci-cargo-deny` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-no-audit: Dependabot opens version-bump PRs, but no CI step fails when a dependency has a known advisory.

## Evidence
- .github/workflows/ci.yml has no audit step; no deny.toml

## Proposed change
- Add src-tauri/deny.toml (advisories=deny, licenses, bans with duplicate warnings).
- Add EmbarkStudios/cargo-deny-action and `pnpm audit --audit-level=high` in CI.
- Add a weekly scheduled run.

## Acceptance criteria / verification
- [ ] Pinning a crate version with a known RUSTSEC advisory fails the job
- [ ] Local: `cargo deny --manifest-path src-tauri/Cargo.toml check`

## Dependencies
None.

## Implementation notes

**Status rationale:** the gate itself is fully in place and verified locally.
It is marked Partial for two reasons. Making the new checks *required* needs
GitHub branch-protection settings, which a human has to change. And existing
advisories are accepted as a commented baseline rather than fixed, because a
full `cargo update` of `src-tauri/Cargo.lock` belongs to the concurrent Rust
lane.

### Change
- `src-tauri/deny.toml` (new):
  - `[advisories]`: vulnerabilities, unsound and yanked crates are errors.
    Unmaintained notices are limited to direct deps (`unmaintained = "workspace"`).
  - `[licenses]`: an allow-list matching the current graph (MIT, Apache-2.0,
    BSD, ISC, MPL-2.0, Unicode-3.0, Zlib, …).
  - `[bans]`: `multiple-versions = "warn"`, wildcards denied.
  - `[sources]`: crates.io only; unknown registries and git sources are denied.
  - cargo-deny reads this file automatically when run from the repo root with
    `--manifest-path src-tauri/Cargo.toml`.
- `.github/workflows/audit.yml` (new, "Dependency Audit"): runs on
  PRs and pushes to `main`, weekly (cron `17 3 * * 1`), and by
  `workflow_dispatch`. It has two jobs:
  - `cargo-deny`: `EmbarkStudios/cargo-deny-action@v2`, `check` on
    `src-tauri/Cargo.toml`.
  - `pnpm audit`: `pnpm audit --audit-level=high`, using pnpm 10.12.3 as in
    ci.yml.
- npm advisories fixed: `zod` ^4.6.7, `smol-toml` ^1.9.1, `vite` ^7.3.7 and
  `postcss` ^8.5.32 were bumped in `package.json`. The lockfile also picked up
  new transitive versions of picomatch, form-data, ws, browserslist, rollup and
  nanoid. That cut high/critical findings from 22 to 4.
- Accepted baselines (each entry has a reason; remove entries once fixed):
  - `pnpm-workspace.yaml` `auditConfig.ignoreGhsas`: 4 GHSAs that come from the
    vitest 2.x chain (vitest UI server, tinypool, and vite 5 via vitest). All are
    dev-only and not shipped. Fixing them needs a major vitest upgrade
    (follow-up).
  - `deny.toml` `[advisories].ignore`: 30 RUSTSEC IDs and 2 yanked crates that
    were in Cargo.lock at introduction. In a scratch copy of `src-tauri/`,
    `cargo update` clears all of them except these:
    - zip 2.x/4.x (RUSTSEC-2026-0367/0368/0369), which needs a zip upgrade.
    - zstd 0.13 / zstd-safe 7 (RUSTSEC-2026-0391/0398/0403), which have no
      semver-compatible fix yet.

  Follow-up: run `cargo update` in the Rust lane, then delete the
  "fix via cargo update" entries.

### Commits
- `4824d75` ci(deps): gate on dependency advisories with cargo-deny and pnpm audit (CCS-018)
- (this ticket-notes commit)

### Verification
- `cargo deny --manifest-path src-tauri/Cargo.toml check` (cargo-deny 0.20.2,
  from `cargo install --locked`): the result was `advisories ok, bans ok,
  licenses ok, sources ok`. 72 duplicate-version warnings remain; they are
  warnings only. The local sandbox could fetch the advisory DB only with
  `[advisories] git-fetch-with-cli = true` (proxy), so later runs used
  `--offline`. CI runners need no such setting.
- Acceptance criterion "pinning a crate with a known RUSTSEC advisory fails
  the job": in a scratch copy of `src-tauri/` with this deny.toml, I added
  `lru = "=0.6.6"`. `cargo deny ... check advisories` then exited 1 with
  `error[vulnerability]: Use after free in lru crate` (RUSTSEC-2021-0130) and
  `advisories FAILED`. Without the pin it printed `advisories ok`.
- `pnpm audit --audit-level=high` exits 0 with pnpm 10.28.0 and with pnpm
  10.12.3 (the CI version), so `auditConfig` in pnpm-workspace.yaml is
  honoured. With one ignore entry removed it exits 1.
- `actionlint .github/workflows/audit.yml .github/workflows/ci.yml`: clean.
  The YAML parses with `yaml.safe_load` (audit.yml, pnpm-workspace.yaml).
- `pnpm typecheck`, `pnpm lint` and `pnpm format:check` pass.
  `pnpm test:unit` gives 525/527. The 2 failures are in
  `tests/integration/App.test.tsx` ("covers basic provider flows via real
  hooks", plus one flaky test). They fail the same way with the original
  lockfile restored, so they are pre-existing and not caused by the bumps.
- No Rust source changed, so `cargo fmt`, `clippy` and `test` were not
  re-run for this ticket.

### Review
An independent review re-checked both acceptance criteria. `cargo deny
--manifest-path src-tauri/Cargo.toml check` (0.20.2) gives `advisories ok,
bans ok, licenses ok, sources ok`. The only warnings are duplicate versions,
and every baseline ignore entry still matches an advisory. A scratch copy
with `lru = "=0.6.6"` exits 1 with RUSTSEC-2021-0130 and `advisories FAILED`.
The pinned `cargo-deny-action@v2` image also ships cargo-deny 0.20.2. Its
entrypoint installs the 1.95 toolchain from `rust-toolchain.toml`, so CI
matches the local run. With pnpm 10.12.3, `pnpm audit --audit-level=high`
exits 0 and `pnpm install --frozen-lockfile` is up to date. `vite build`,
typecheck, lint and format:check pass. `pnpm test:unit` passed 527/527 on
two consecutive runs, so the earlier App.test.tsx failures are flaky, not a
regression. Fixed in review: the `pnpm-workspace.yaml` comment pointed to
ci.yml, but the gate lives in `.github/workflows/audit.yml`. Status stays
Partial: making the checks required in branch protection still needs a human.

### Remaining (needs a human or another lane)
- Mark the `cargo-deny` and `pnpm audit` checks as required in branch
  protection for `main`.
- Refresh `src-tauri/Cargo.lock` with `cargo update` and prune the baseline.
  Upgrade zip; upgrade vitest to ≥3.2.6 or 4 and drop the 4 ignored GHSAs.
