# CCS-032: [ci] Document or automate promoting a prerelease to latest for the updater

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-prerelease-promotion` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-prerelease-updater. Every release is created with prerelease: true, but the updater reads releases/latest, which skips prereleases. Promotion is a manual step that is not documented anywhere.

## Evidence
- release.yml:612; src-tauri/tauri.conf.json:65

## Proposed change
- Document `gh release edit vX.Y.Z --prerelease=false --latest` in docs/dev/RELEASING.md.
- Add a workflow_dispatch `promote-release` job.
- Optionally set prerelease from `contains(github.ref_name, '-')`.

## Acceptance criteria / verification
- [ ] RELEASING.md exists and is linked from CONTRIBUTING.md
- [ ] On a fork, after promotion, `curl -sIL .../releases/latest/download/latest.json` redirects to the new tag

## Dependencies
None.

## Implementation notes

**Status rationale:** Partial — everything that can be done in-repo is done; the
second acceptance item (a real promotion on a fork followed by `curl -sIL`)
needs a GitHub fork with a published release and cannot run in this sandbox.

### Summary
- `docs/dev/RELEASING.md`: maintainer release checklist — why promotion is needed
  (prerelease + `releases/latest` updater endpoint), cut / smoke-test / promote /
  verify / roll back, including `gh release edit vX.Y.Z --prerelease=false --latest`.
  Linked from `CONTRIBUTING.md` (new "Releasing" / "发布流程" sections).
- `.github/workflows/promote-release.yml`: `workflow_dispatch` (input `tag`)
  `promote-release` job, `contents: write`, concurrency group
  `promote-release-<tag>` (cancel-in-progress: false; see Review). Runs the script, then curls
  `releases/latest/download/latest.json` and requires a `Location:` to
  `releases/download/<tag>/latest.json` (retries 5x). Input is passed via `env`,
  never interpolated into `run:`.
- `scripts/promote-release.sh <repo> <tag>`: refuses non-stable tags (`-beta`/`-rc`
  etc.), missing or draft releases, and releases whose `latest.json` asset is
  missing or has a different `version`; then `gh release edit --prerelease=false
  --latest` and checks `gh api repos/<repo>/releases/latest` returns the tag.
- `release.yml`: comment at `prerelease: true` pointing to the promotion flow.
  The optional "set prerelease from `contains(github.ref_name, '-')`" was not
  adopted: keeping every build a prerelease preserves the manual smoke-test gate
  before the updater ships it (a human decision may change this later).

### Files
- `.github/workflows/promote-release.yml` (new)
- `.github/workflows/release.yml`
- `scripts/promote-release.sh` (new)
- `docs/dev/RELEASING.md` (new)
- `CONTRIBUTING.md`
- `tests/ci/promoteRelease.test.ts` (new)

### Commits
- a82ad44 ci(release): add a promote-release workflow and document promotion (CCS-032)

### Verification
- `pnpm exec vitest run tests/ci` → 6 files, 66 tests passed (new
  `promoteRelease.test.ts`: 14 tests — script against a stub `gh` for the happy
  path, non-stable tags, missing/draft release, missing/mismatched latest.json,
  releases/latest not switching; plus wiring checks for the workflow, concurrency
  group, updater endpoint, RELEASING.md and the CONTRIBUTING.md link). The suite
  fails before this change (files absent).
- `pnpm typecheck` → clean; `pnpm format:check` → clean; `pnpm test:unit` → 85
  files, 596 tests passed.
- YAML: `python3 -c "import yaml,sys;yaml.safe_load(open(sys.argv[1]))"` passes for
  promote-release.yml and release.yml. actionlint / shellcheck not installed.
- actionlint v1.7.12 with shellcheck 0.11.0 on PATH (CCS-217): `actionlint .github/workflows/*.yml` → exit 0 with the committed `.github/actionlint.yaml`;
  `shellcheck scripts/promote-release.sh` → exit 0, no findings.
- Redirect check validated against upstream: `curl -sSIL
  https://github.com/farion1231/cc-switch/releases/latest/download/latest.json`
  first `Location:` is `.../releases/download/v4.0.21/latest.json`, matching the
  pattern the workflow asserts.

### Review
Verified both commits against the spec: RELEASING.md exists, documents
`gh release edit vX.Y.Z --prerelease=false --latest` and is linked from both
language sections of CONTRIBUTING.md; the workflow_dispatch job, the script's
refusal checks and the redirect check are correct (`latest.json` `version` is
`${TAG#v}` in generate-latest-json.sh, matching the script). One defect fixed:
the job originally joined release.yml's `release-publish-<tag>` group. In
GitHub Actions a newly queued job cancels any job already *pending* in the same
group, so a promotion queued while `assemble-latest-json` was waiting (e.g. a
second promote dispatched during the first) could cancel the latest.json
upload, and a pending promotion was itself cancelled rather than "waiting" when
assemble queued. The job now uses its own `promote-release-<tag>` group; an
early promotion is refused by the script's latest.json check instead. Docs and
the wiring test were updated. Status stays Partial (fork check still manual).

### Remaining
- [ ] On a fork: tag a release, run **Promote Release** with the tag, and confirm
  `curl -sIL https://github.com/<fork>/releases/latest/download/latest.json`
  redirects to the new tag (the workflow's "Verify updater endpoint" step does
  this automatically).

### Remaining after triage

- Blocked: AC2: after promotion, `curl -sIL .../releases/latest/download/latest.json` redirects to the new tag. Needs a fork with a published release and a manual promote-release.yml run.
- Done in [CCS-217](../P0-safety-net/CCS-217-p0-ci-actionlint-shellcheck.md): actionlint with shellcheck passes and its config is committed.

## Owner decision: GitHub Actions checks skipped

By the repository owner's decision on 2026-10-31, the following checks that need a GitHub Actions run are **skipped, not verified**. No green Actions run has been observed for them; they no longer block this ticket.

- AC2: on a fork, tag a release (release.yml tag run) and run the Promote Release workflow (promote-release.yml, workflow_dispatch), then confirm `curl -sIL https://github.com/<fork>/releases/latest/download/latest.json` redirects to the new tag. This needs a real GitHub Actions run; the workflow's 'Verify updater endpoint' step does the curl check itself.

With these skipped, nothing else blocks this ticket, so it is set to **Done**.
