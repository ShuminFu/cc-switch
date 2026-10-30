# CCS-218: [ci-tests] Add executable regression tests for the release gate, signing-key validation and SHA256SUMS

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci-tests`

> **Ticket key:** `p0-ci-release-regression-tests` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-023, CCS-029, CCS-033). Source tickets: [CCS-023](../P0-safety-net/CCS-023-p0-ci-release-gated.md), [CCS-029](../P0-safety-net/CCS-029-p0-ci-release-secrets-env.md), [CCS-033](../P0-safety-net/CCS-033-p0-ci-release-hygiene.md).

## Problem
Three release-path behaviours can only be proven end to end on GitHub Actions, and today their local guards are text checks:
- CCS-023: `tests/ci/releaseHygiene.test.ts:67-74` only checks `needs: [ci, check-versions]` on `release`, and `tests/ci/e2eSmoke.test.ts:81` checks `needs: release` on publish-release. Nothing stops an `always()` job condition, a tag-skipped CI job or a `continue-on-error` from letting a red CI through.
- CCS-029: the "Validate Tauri signing key" step in `release.yml` (~194-215) was checked by hand; `tests/ci/releaseSecrets.test.ts` checks text and placement but never runs the script.
- CCS-033: `scripts/generate-sha256sums.sh` is unit-tested (`releaseHygiene.test.ts:104-130`) but not in the publish-release sparse-checkout layout, nor against the `subject-checksums` format attest-build-provenance expects.

## Proposed change
- **Release gate** (`tests/ci/releaseGate.test.ts`, parse YAML): (a) ci.yml `on:` includes `workflow_call`; (b) release.yml `ci` job is `uses: ./.github/workflows/ci.yml`; (c) assemble-latest-json `needs: publish-release`; (d) no job-level `if:` on release, publish-release or assemble-latest-json uses `always()`, `!cancelled()` or `failure()`; (e) no ci.yml job has an `if:` that skips on tag push / `workflow_call` (e.g. `github.ref == 'refs/heads/main'`, `github.event_name == 'pull_request'`); (f) frontend, backend, msrv and tauri-build-smoke have no `continue-on-error: true` (only coverage-frontend/coverage-rust may).
- **Signing key** (`tests/ci/releaseSigningKeyValidate.test.ts`): extract the step's `run:` body and run it with `bash -c` via `spawnSync` under a controlled env. Cases: canonical key (base64 of `untrusted comment: minisign encrypted secret key\n<line2>\n`) exits 0 and never echoes the key; empty/unset exits 1 with the `Secret 为空` message; raw two-line text exits 1; garbage or the bare second line exits 1.
- **SHA256SUMS**: in a temp dir, build `release-assets/` with fake installer/.sig/latest.json files and `src/scripts/generate-sha256sums.sh` as the sparse checkout lays it out; run it, then `sha256sum -c SHA256SUMS --ignore-missing`; assert each line matches `/^[0-9a-f]{64}  [^/]+$/`.

## Acceptance criteria / verification
- [ ] `pnpm vitest run tests/ci` passes with the three new suites.
- [ ] Each new guard fails when the guarded property is broken in a scratch copy (e.g. add `if: always()` to publish-release; drop the base64 check from the validate step).
- [ ] `pnpm test:unit` passes.

## Dependencies
None.
