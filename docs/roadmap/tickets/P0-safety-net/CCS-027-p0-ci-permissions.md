# CCS-027: [ci] Add a minimal permissions block to ci.yml

**Status:** Partial · **Kind:** security · **Labels:** `security`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-permissions` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-no-permissions-block. ci.yml uses the repo-default GITHUB_TOKEN scope, which may be write-all, on push to main and same-repo PRs.

## Evidence
- .github/workflows/ci.yml has no permissions key (the other workflows set one)

## Proposed change
Add a top-level `permissions: contents: read`.

## Acceptance criteria / verification
- [x] `actionlint .github/workflows/ci.yml`
- [ ] The job log's GITHUB_TOKEN Permissions lists only Contents: read and Metadata: read

## Dependencies
None.

## Implementation notes

**Summary:** Added a top-level `permissions: contents: read` block to
`.github/workflows/ci.yml`, so its GITHUB_TOKEN no longer inherits the
repository default. No job in ci.yml needs more: checkout, cache and
`upload-artifact` work with contents: read (artifact upload uses the runtime
token, not GITHUB_TOKEN scopes). `release.yml` already calls ci.yml with
`permissions: contents: read`, which matches, so the reusable call stays valid.
Added `tests/ci/workflowPermissions.test.ts`, which checks that every workflow
declares `permissions` (top-level or, as in `claude.yml`, per job) and that
ci.yml grants exactly `contents: read` with no job-level overrides or `write`
grants.

**Files:** `.github/workflows/ci.yml`, `tests/ci/workflowPermissions.test.ts`

**Commits:** `904b182` (code + test); ticket notes in the follow-up commit.

**Verification:**
- `python3 -c "import yaml,sys;yaml.safe_load(open(sys.argv[1]))" .github/workflows/ci.yml` passes
- Regression: with the old ci.yml (`git show HEAD~:.github/workflows/ci.yml`) in place,
  `npx vitest run tests/ci/workflowPermissions.test.ts` failed 2 ci.yml cases; with the change, they pass
- `pnpm typecheck` passes; `pnpm test:unit` gives 81 files / 545 tests passed; `pnpm format:check` is clean

**Remaining (why Partial):**
- [x] `actionlint .github/workflows/ci.yml`: done in review (see below).
- [ ] Check that the GITHUB_TOKEN Permissions section of the "Set up job" log shows only
  Contents: read and Metadata: read. This needs a real GitHub Actions run on a PR or on main.

**Review:** `~/go/bin/actionlint` (v1.7.7) was available after all:
`actionlint .github/workflows/ci.yml` exits 0. (A full `actionlint` run only
reports the pre-existing `windows-11-arm` runner label in release.yml, which
this actionlint version does not know about.) No job in ci.yml needs more than
contents: read. Tightened the guard test: before, any `permissions:` line
anywhere in a file passed, so a workflow with no top-level block and one
unscoped job would still pass. Now a workflow without top-level permissions
must declare them on every job. Checked that it fails when an extra
job without permissions is added to claude.yml, and fails on the old ci.yml.
Status stays Partial: the job-log token check needs a real GitHub Actions run.

### Remaining after triage

- Blocked: The 'Set up job' log lists only Contents: read and Metadata: read. Needs a real GitHub Actions run (CI triggers only on main/PRs; the repo has no Actions history); no local tool reproduces GitHub's token scopes.
- Follow-ups: none; nothing left here can be coded or verified locally.
