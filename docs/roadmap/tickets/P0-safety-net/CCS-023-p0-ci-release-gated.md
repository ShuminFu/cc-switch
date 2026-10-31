# CCS-023: [ci] Require green CI on the tag SHA before release builds

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:ci`

> **Ticket key:** `p0-ci-release-gated` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-release-not-gated-on-ci. release.yml runs on a v* tag push without CI and runs no tests, so a broken commit can publish a prerelease.

## Evidence
- .github/workflows/release.yml:3-6 trigger, :16 release job without needs; ci.yml:3-7 only runs on PRs and main

## Proposed change
Add `workflow_call:` to ci.yml. In release.yml, add a `ci` job (`uses: ./.github/workflows/ci.yml`) and set `needs: ci` on `release`. Add required reviewers on the `release` environment.

## Acceptance criteria / verification
- [x] `actionlint` passes
- [ ] On a fork, a test tag on a commit with a failing test skips release and publish-release

## Dependencies
None.

## Implementation notes
**Summary.** `ci.yml` now also triggers on `workflow_call:`. `release.yml` has a new `ci` job (`uses: ./.github/workflows/ci.yml`, `permissions: contents: read`) that runs the full CI suite (frontend checks, the Linux/Windows/macOS backend matrix, and the report-only coverage jobs) on the tag SHA. `release` now has `needs: ci`. `publish-release` needs `release` and `assemble-latest-json` needs `publish-release`, and none of them uses `always()`. So a red CI run skips the whole release chain. The report-only coverage jobs keep `continue-on-error: true`, so they never block a release.

**Files.** `.github/workflows/ci.yml`, `.github/workflows/release.yml`, this ticket.

**Commits.** See `git log --grep CCS-023` (code and ticket in one commit).

**Verification.**
- `actionlint` v1.7.7 (built locally with `go install`): run as `actionlint -config-file <cfg> -shellcheck= .github/workflows/ci.yml .github/workflows/release.yml` → no findings, exit 0. `<cfg>` only lists `windows-11-arm` as a known runner label, because v1.7.7 predates that GitHub-hosted label. shellcheck is not installed, so the shell-script checks were off.
- actionlint v1.7.12 with shellcheck 0.11.0 on PATH (CCS-217): `actionlint .github/workflows/*.yml` → exit 0 with the committed `.github/actionlint.yaml`; the only shellcheck findings (SC2086, unquoted `$GITHUB_OUTPUT` in the pnpm-store steps of ci.yml and release.yml) were fixed there.
- `python3 -c "import yaml,sys;yaml.safe_load(open(sys.argv[1]))"` parses both files. A structural check confirms that the ci triggers are `pull_request, push, workflow_call` and that the needs chain is `ci -> release -> publish-release -> assemble-latest-json`.

**Remaining (cannot be done from this environment).**
- Fork test: push a `v*` tag on a commit with a failing test and confirm that `release` and `publish-release` are skipped. This needs a GitHub fork with Actions enabled.
- Required reviewers on the `release` environment: a repository setting (Settings → Environments → `release` → Required reviewers). A maintainer has to set it.

### Remaining after triage

- Blocked: Fork test: a v* tag on a red commit skips release, publish-release and assemble-latest-json. Needs a real Actions run on a fork (act does not emulate reusable workflows or needs-skip).
- Blocked: Required reviewers on the `release` environment. Needs a maintainer to change GitHub environment settings.
- Done in [CCS-217](../P0-safety-net/CCS-217-p0-ci-actionlint-shellcheck.md): actionlint with shellcheck passes and its config is committed.
- Follow-up [CCS-218](../P0-safety-net/CCS-218-p0-ci-release-regression-tests.md): Add executable regression tests for the release gate, signing-key validation and SHA256SUMS.

## Owner decision: GitHub Actions checks skipped

By the repository owner's decision on 2026-10-31, the following checks that need a GitHub Actions run are **skipped, not verified**. No green Actions run has been observed for them; they no longer block this ticket.

- Fork test: a v* tag on a commit with a failing test must skip release, publish-release and assemble-latest-json. This needs a real GitHub Actions run on a fork.

### Still blocked by

- Required reviewers on the `release` environment (Settings > Environments > release > Required reviewers). **Needs:** A maintainer has to change this GitHub repository/environment setting.
