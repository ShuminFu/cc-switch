# CCS-217: [ci] Run actionlint with shellcheck on every workflow and release script, and commit its config

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

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
- [ ] `actionlint .github/workflows/*.yml` exits 0 with shellcheck enabled.
- [ ] `shellcheck scripts/promote-release.sh scripts/generate-sha256sums.sh` exits 0.
- [ ] `pnpm vitest run tests/ci tests/scripts` passes (promoteRelease 14/14, checkProxyFixture 13 cases).

## Dependencies
None.
