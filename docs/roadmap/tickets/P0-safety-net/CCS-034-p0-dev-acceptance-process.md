# CCS-034: [process] Add the ACCEPTANCE playbook, the fixture-required PR rule and the fix-commit KPI

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:process`

> **Ticket key:** `p0-dev-acceptance-process` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p0-golden-testkit-chat-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The plan's process rules need to be written down and enforced. They are: every proxy bug fix adds a fixture first, structural PRs keep the old path callable, and fix commits per release are tracked as a KPI. Contributors also lack a how-to for adding an app type (CI idea 10).

## Proposed change
- Add docs/dev/ACCEPTANCE.md covering "add an app" and "fix a proxy bug". Add a CLAUDE.md/CONTRIBUTING pointer.
- Add .github/pull_request_template.md with a fixture checkbox.
- Add a labeler plus a check: a change under src-tauri/src/proxy/** needs a tests/fixtures/proxy change or the `no-fixture-needed` label.
- Add a scripts/kpi-fix-commits.sh baseline over the last 3 releases.

## Acceptance criteria / verification
- [x] A test PR touching proxy/ without a fixture or label fails the check
- [x] The baseline KPI is recorded in docs/dev

## Dependencies
p0-golden-testkit-chat-gemini

## Implementation notes

**Summary.** The process rules and two playbooks ("fix a proxy bug", "add an
app") are written down in `docs/dev/ACCEPTANCE.md`, with pointers from
CONTRIBUTING.md (EN and ZH PR guidelines and checklists) and from
RELEASING.md (paste the KPI row into the release notes). There is no
CLAUDE.md in the repo, so CONTRIBUTING.md is the only pointer.

- **Fixture rule.** The new `.github/workflows/proxy-fixture.yml` runs on
  `pull_request` to `main` (opened, synchronize, reopened, labeled,
  unlabeled) with `contents: read` and SHA-pinned actions. It runs
  `scripts/check-proxy-fixture.mjs`, which takes the `base...head` diff
  (merge base) and fails when files under `src-tauri/src/proxy/` changed but
  nothing under `src-tauri/tests/fixtures/proxy/` did, unless the PR has the
  `no-fixture-needed` label. Labels come in through
  `PR_LABELS=toJSON(...labels.*.name)` in `env`, never interpolated into the
  script. The fixture path is the real corpus location
  (`src-tauri/tests/fixtures/proxy/`). The ticket's `tests/fixtures/proxy` is
  shorthand for it.
- **Labeler.** A `fixtures` label for `src-tauri/tests/fixtures/proxy/**`.
  `no-fixture-needed` is set by hand only and is kept out of labeler.yml,
  because `sync-labels: true` would remove it. A test guards this.
- **PR template.** A fixture checkbox and a "keep the old path callable"
  checkbox, both bilingual.
- **KPI.** `scripts/kpi-fix-commits.sh [-C repo] [--ref ref] [N]` prints a
  Markdown table of commits, fix commits, fix % and proxy fix commits for the
  last N (default 3) stable `vX.Y.Z` release windows, plus an `unreleased`
  row. This clone is shallow and has no tags, and the fork remote has none
  either, so the baseline was generated against a blobless clone of
  upstream farion1231/cc-switch at this branch's base `6d2ee24`, which sits
  between v3.16.5 and v3.17.0. Baseline: v3.16.3 to v3.16.5 have 49 fix
  commits out of 156 (31%) and 9 proxy fix commits, 3.0 per release. The
  baseline is recorded in ACCEPTANCE.md.

**Files.** `.github/workflows/proxy-fixture.yml` (new),
`.github/labeler.yml`, `.github/pull_request_template.md`,
`scripts/check-proxy-fixture.mjs` + `.d.mts` (new),
`scripts/kpi-fix-commits.sh` (new), `tests/scripts/checkProxyFixture.test.ts`
(new), `tests/scripts/kpiFixCommits.test.ts` (new), `docs/dev/ACCEPTANCE.md`
(new), `docs/dev/RELEASING.md`, `CONTRIBUTING.md`.

**Commits.** a4c9d43 (implementation); this ticket update follows in a
separate commit.

**Verification.**
- `pnpm vitest run tests/scripts/checkProxyFixture.test.ts`: 13 passed. It
  builds a throwaway git repo with a test PR that touches
  `src-tauri/src/proxy/forwarder.rs`. The script exits 1 without a fixture
  or label and names the file, exits 0 with a fixture or with
  `no-fixture-needed`, and ignores proxy commits that landed on main after
  the branch point.
- `pnpm vitest run tests/scripts/kpiFixCommits.test.ts`: 3 passed (window
  counts, prerelease tags skipped, `--ref`, error on too few tags).
- `pnpm typecheck`: clean. `pnpm format:check`: clean. `pnpm test:unit`: 89
  files, 638 tests passed.
- YAML: `python3 -c "import yaml..."` parses proxy-fixture.yml and
  labeler.yml. `bash -n scripts/kpi-fix-commits.sh`: OK.
- `scripts/kpi-fix-commits.sh -C <upstream clone> --ref 6d2ee24` produced
  the baseline table. Run in this shallow clone, it exits 1 with a "fetch
  tags" hint.
- No Rust files changed, so cargo fmt, clippy and test were not needed.
- actionlint v1.7.12 with shellcheck 0.11.0 on PATH (CCS-217): `actionlint .github/workflows/*.yml` → exit 0 with the committed `.github/actionlint.yaml`
  (covers proxy-fixture.yml, including the `toJSON(...labels.*.name)`
  expression, and labeler.yml: no findings); `shellcheck
  scripts/kpi-fix-commits.sh` → exit 0.

**Remaining (needs a human or GitHub settings).**
- Make **Proxy fixture required** a required status check in the branch
  protection rule for `main`. Until then a failing check does not block
  merging.
- Create the `no-fixture-needed` and `fixtures` labels in the repository.
  The labeler creates `fixtures` on first use.

### Remaining after triage

- Blocked: Make 'Proxy fixture required' a required check on main. Needs a repo admin (branch protection).
- Blocked: Create the `no-fixture-needed` and `fixtures` labels. Needs repo write access and a human decision.
- Blocked: See the proxy-fixture workflow pass/fail on a real PR. Needs a real GitHub Actions run (CI triggers only on main/PRs; the repo has no Actions history) (the 13-case local test stands in).
- Done in [CCS-217](../P0-safety-net/CCS-217-p0-ci-actionlint-shellcheck.md): actionlint with shellcheck passes and its config is committed.
