# CCS-031: [ci] Never cancel an in-progress release publish

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-release-concurrency` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-release-cancel-in-progress. A workflow-level cancel-in-progress can cut publish-release or assemble-latest-json mid-upload. Cancelling during the build matrix is harmless.

## Evidence
- .github/workflows/release.yml:11-13

## Proposed change
- Use job-level concurrency: keep cancel-in-progress: true on the build matrix.
- Put publish-release and assemble-latest-json in a `release-publish-<ref>` group with cancel-in-progress: false.

## Acceptance criteria / verification
- [x] actionlint passes
- [ ] Fork test: a re-pushed tag during publish waits rather than cancelling

## Dependencies
None.

## Implementation notes

**Summary.** Removed the workflow-level `concurrency` (`release-<ref>`, `cancel-in-progress: true`) from `release.yml` and moved concurrency to the jobs:

| Job | Group | cancel-in-progress |
| --- | --- | --- |
| `ci` (reusable call) | `release-ci-${{ github.ref_name }}` | `true` |
| `release` (build matrix) | `release-build-${{ github.ref_name }}-${{ matrix.os }}` | `true` |
| `publish-release` | `release-publish-${{ github.ref_name }}` | `false` |
| `assemble-latest-json` | `release-publish-${{ github.ref_name }}` | `false` |

The build group includes `matrix.os` (each matrix entry has a unique `os`) so the matrix entries of one run don't cancel each other. A re-pushed tag still cancels the older run's CI/builds (nothing has been published yet). In the publish group a newer run's job waits for the running publish. GitHub keeps only one pending job per group, so if a third run queues, the older *pending* job is replaced, but a running job is never cancelled. `ci.yml` keeps its own workflow-level `ci-<ref>` cancel group, which only affects CI jobs.

**Files.**
- `.github/workflows/release.yml`
- `tests/ci/releaseConcurrency.test.ts` (new regression test: no workflow-level concurrency; both publish jobs in `release-publish-<ref>` with `cancel-in-progress: false`; build matrix cancellable with a per-`matrix.os` group; every job declares concurrency)

**Commits.** `663b3a5` (code + test), plus the follow-up commit that records these notes.

**Verification.**
- `actionlint -ignore 'label "windows-11-arm" is unknown' .github/workflows/*.yml` (actionlint v1.7.7, built locally): exit 0. The ignored error was already there before this change: v1.7.7 doesn't know the `windows-11-arm` hosted runner label.
- `python3 -c "import yaml,sys;yaml.safe_load(open(sys.argv[1]))" .github/workflows/release.yml`: parses; no top-level `concurrency`; job groups as in the table above.
- `npx vitest run tests/ci/releaseConcurrency.test.ts` against the old `release.yml` (from HEAD~): 5 failed. Against the new file: all pass. `tests/ci/`: 5 files, 48 tests passed.
- `pnpm typecheck`: pass. `pnpm test:unit`: 84 files, 578 tests passed. `pnpm format:check`: pass.

**Remaining (cannot be done here).** The "Fork test" item still needs a GitHub fork with the release secrets/environment: push a `v*` tag, re-push it (force-update) while `publish-release` is running, and confirm the new run's `publish-release` shows "Waiting for a pending job in this concurrency group" rather than cancelling the running one. This is why the status is Partial.

**Review.** I checked the change against the spec. The workflow-level `cancel-in-progress` is gone. `publish-release` and `assemble-latest-json` share `release-publish-<ref>` with `cancel-in-progress: false`. The build matrix and `ci` can still be cancelled. Job-level `concurrency` is allowed on a reusable-workflow caller job, and the `matrix` context is available in `jobs.<id>.concurrency`. `actionlint` v1.7.7 only reports the existing `windows-11-arm` label error. One gap: the per-OS build group only works if every `matrix.os` is unique. Two entries with the same `os` would silently cancel each other. The test now asserts that the matrix `os` values are unique, and I checked that it fails when an `os` is duplicated. A residual risk comes from the shared publish group. If the older run's `assemble-latest-json` queues while the newer run's `publish-release` is still pending, GitHub can replace that pending publish (one pending job per group). Then the newer run's publish is dropped instead of waiting. No running job is ever cancelled, so this stays within the ticket. Covering it fully needs the fork test. Status stays Partial until that test is done.
