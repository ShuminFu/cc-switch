# CCS-043: [ci] Remove the invalid stale.yml input and implement a real popular-issue exemption

**Status:** Partial · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-stale-workflow` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-stale-invalid-input. `exempt-issue-created-after` is not an actions/stale input, so the "3+ reactions" exemption never applies and popular issues get closed.

## Evidence
- .github/workflows/stale.yml:40-41

## Proposed change
- Remove the line and its comment.
- Add a `popular` label to exempt-issue-labels, applied by a small github-script step to issues with 3 or more reactions.

## Acceptance criteria / verification
- [ ] `actionlint` passes (done locally); a workflow_dispatch run with debug-only: true has no Unexpected input warning and skips labelled issues

## Dependencies
None.

## Implementation notes

**Summary.** Removed `exempt-issue-created-after` (not an `actions/stale` input) and its misleading comment. Added a `Label popular issues` step (`actions/github-script@ed59741… # v8.0.0`, SHA-pinned like the other actions) that runs before `actions/stale`, lists open issues (PRs skipped) and adds the `popular` label to any with `reactions.total_count >= 3` that lack it (GitHub creates the label on first use). `popular` is added to `exempt-issue-labels`. `workflow_dispatch` gains a boolean `debug-only` input; it is exposed as job env `DEBUG_ONLY` and passed both to `actions/stale` `debug-only` and to the labelling script, which then only logs what it would label. Scheduled runs have no inputs, so `DEBUG_ONLY` is `false`. Label name and threshold live in job env (`POPULAR_LABEL`, `POPULAR_MIN_REACTIONS`).

**Files.**
- `.github/workflows/stale.yml`
- `tests/ci/staleWorkflow.test.ts` (new): checks every `actions/stale` `with:` key against the inputs declared in that action's `action.yml` at the pinned SHA (v10.4.0), checks that `popular` is exempt, that labelling runs before stale, and that `debug-only` is wired through. It also pulls the inline github-script out of the workflow and runs it against a mocked `github`/`context`/`core`: it labels only open non-PR issues with 3+ reactions that are not labelled yet, and makes no writes in a debug-only run.

**Commits.** `e0a8b49` (workflow + test); this ticket update is a follow-up commit.

**Verification.**
- `~/go/bin/actionlint` (v1.7.12) on all workflows: no findings. Note: actionlint does not check inputs of SHA-pinned actions (the old file also passed), which is why the vitest check exists.
- `python3 -c "import yaml,sys;yaml.safe_load(open(sys.argv[1]))" .github/workflows/stale.yml`: parses.
- `npx vitest run tests/ci/staleWorkflow.test.ts`: 5/5 pass; against the previous `stale.yml`, all 5 fail (regression proven).
- `pnpm typecheck`: passes. `pnpm format:check`: passes.
- `pnpm test:unit`: 644 passed, 2 failed, both in `tests/integration/App.test.tsx` ("covers basic provider flows via real hooks", a 5s timeout). That file fails the same way when run alone, and this change does not touch it (workflow YAML and a new isolated test only).

**Remaining (cannot be done locally).** The second half of the acceptance criterion needs a real GitHub run: trigger `Close Stale Issues` with `workflow_dispatch` and `debug-only: true`, then confirm the log has no `Unexpected input(s)` warning and that issues labelled `popular`/`security`/`performance` are reported as skipped. This session has no GitHub API access to the repository.

### Remaining after triage

- Blocked: workflow_dispatch run of 'Close Stale Issues' with debug-only: true, checking for no 'Unexpected input(s)' warning and that popular/security/performance issues are skipped. Needs a real Actions run with issues:write; nothing more is codable.
- Follow-ups: none; nothing left here can be coded or verified locally.
