# CCS-217: [ci] Run actionlint with shellcheck on every workflow and release script, and commit its config

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-actionlint-shellcheck` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-023, CCS-032, CCS-033, CCS-034, CCS-001). Source tickets: [CCS-023](../P0-safety-net/CCS-023-p0-ci-release-gated.md), [CCS-032](../P0-safety-net/CCS-032-p0-ci-prerelease-promotion.md), [CCS-033](../P0-safety-net/CCS-033-p0-ci-release-hygiene.md), [CCS-034](../P0-safety-net/CCS-034-p0-dev-acceptance-process.md), [CCS-001](../P0-safety-net/CCS-001-epic-p0.md).

## Problem
Four P0 tickets list an actionlint/shellcheck run as unfinished: CCS-023 (box "`actionlint` passes" unticked; earlier runs had shellcheck off and a throwaway config), CCS-032 (`promote-release.yml`, `scripts/promote-release.sh`), CCS-033 (`ci.yml`/`release.yml`, `scripts/generate-sha256sums.sh`) and CCS-034 (`proxy-fixture.yml`, `labeler.yml`; still listed in the CCS-001 epic table at line 58). actionlint is now at `~/go/bin/actionlint` and `actionlint .github/workflows/*.yml` exits 0 without shellcheck, but nothing is recorded and the `windows-11-arm` label workaround is not in the repo.

## Proposed change
- Install shellcheck (`pip install shellcheck-py` into a scratch venv, or apt).
- Add `.github/actionlint.yaml` with `self-hosted-runner: labels: [windows-11-arm]` if the installed actionlint version still needs it, so anyone can reproduce the run.
- Run `actionlint .github/workflows/*.yml` with shellcheck on PATH, and `shellcheck scripts/promote-release.sh scripts/generate-sha256sums.sh scripts/kpi-fix-commits.sh`. Fix real findings in `run:` blocks (check-versions job, publish-release, the SHA256SUMS/attest steps, proxy-fixture's `toJSON(...labels.*.name)` expression).
- Record the actionlint/shellcheck versions and the result under Verification in CCS-023, CCS-032, CCS-033 and CCS-034; tick the CCS-023 box; drop "run actionlint" from CCS-034's Remaining list and the CCS-001 epic table row.

## Acceptance criteria / verification
- [x] `actionlint .github/workflows/*.yml` exits 0 with shellcheck enabled.
- [x] `shellcheck scripts/promote-release.sh scripts/generate-sha256sums.sh` exits 0.
- [x] `pnpm vitest run tests/ci tests/scripts` passes (promoteRelease 14/14, checkProxyFixture 13 cases).

## Dependencies
None.

## Implementation notes

**Summary.** With shellcheck on PATH, actionlint found only SC2086 (info):
`echo "path=$(pnpm store path --silent)" >> $GITHUB_OUTPUT` in three
pnpm-store steps of `ci.yml` and one in `release.yml`. The redirect target is
now quoted. The other areas the ticket named (check-versions job,
publish-release, the SHA256SUMS/attest steps, proxy-fixture's
`toJSON(github.event.pull_request.labels.*.name)` expression, labeler.yml)
had no findings. actionlint v1.7.12 knows `windows-11-arm`, but v1.7.7 (used
for CCS-023) does not, so `.github/actionlint.yaml` lists it as a runner label
to keep older versions reproducible. `tests/ci/actionlintConfig.test.ts`
checks every workflow for unquoted redirects to `$GITHUB_OUTPUT` / `ENV` /
`STEP_SUMMARY` / `PATH` and checks that the config declares `windows-11-arm`.
It fails on the previous `ci.yml` (3 matches) and `release.yml` (1 match).
Verification notes were recorded in CCS-023 (box ticked), CCS-032, CCS-033
and CCS-034 ("actionlint was not run" removed), and the CCS-001 epic row no
longer lists "run actionlint".

**Files.** `.github/actionlint.yaml` (new), `.github/workflows/ci.yml`,
`.github/workflows/release.yml`, `tests/ci/actionlintConfig.test.ts` (new),
CCS-001/023/032/033/034 ticket files, this ticket.

**Commits.** e26e590 (code and test); the ticket-doc commit follows it
(`git log --grep CCS-217`).

**Verification.**
- Tools: actionlint v1.7.12 (`~/go/bin/actionlint`), shellcheck 0.11.0
  (`pip install shellcheck-py` in a scratch venv).
- `actionlint .github/workflows/*.yml` → exit 0; `actionlint -verbose` no
  longer reports the shellcheck rule as disabled. Before the fix: 4 SC2086
  findings, exit 1.
- `shellcheck scripts/promote-release.sh scripts/generate-sha256sums.sh` →
  exit 0; `shellcheck scripts/*.sh` (including kpi-fix-commits.sh) → exit 0.
- YAML parses (`yaml.safe_load`) for `.github/actionlint.yaml` and all eight
  workflows.
- `pnpm vitest run tests/ci tests/scripts` → 21 files, 153 tests passed;
  promoteRelease + checkProxyFixture → 27 tests (14 + 13) passed.
- `pnpm test:unit` → 168 files, 1587 tests passed; `pnpm typecheck` clean;
  `pnpm format:check` clean. No Rust files changed.
- Not done: actionlint is not wired into CI as a job; the ticket did not ask
  for it.
