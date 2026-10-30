# CCS-218: [ci-tests] Add executable regression tests for the release gate, signing-key validation and SHA256SUMS

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci-tests`

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
- [x] `pnpm vitest run tests/ci` passes with the three new suites.
- [x] Each new guard fails when the guarded property is broken in a scratch copy (e.g. add `if: always()` to publish-release; drop the base64 check from the validate step).
- [x] `pnpm test:unit` passes.

## Dependencies
None.

## Implementation notes

**Summary.** Three new vitest suites under `tests/ci/` turn the release-path text checks into executable guards. Workflows are parsed with `js-yaml` (added as a devDependency together with `@types/js-yaml`; it was already in the lockfile transitively at 4.3.2).

- `tests/ci/releaseGate.test.ts`: ci.yml `on:` has `workflow_call`; release.yml `ci` job `uses: ./.github/workflows/ci.yml`; `release` needs `ci` + `check-versions`, `publish-release` needs `release`, `assemble-latest-json` needs `publish-release`; no job-level `if:` on ci/check-versions/release/publish-release/assemble-latest-json uses `always()`, `!cancelled()` or `failure()`; no ci.yml job has a ref/event-based `if:` (and the four gating jobs have no job `if:` at all); frontend/backend/msrv/tauri-build-smoke have no job- or step-level `continue-on-error` (only coverage-frontend/coverage-rust may).
- `tests/ci/releaseSigningKeyValidate.test.ts`: extracts the `Validate Tauri signing key` step's `run:` body from the parsed workflow and executes it with `bash -c` under a minimal env (`PATH` + key). Canonical base64 key → exit 0, key/second line never in output; unset/empty → exit 1 with `Secret 为空`; raw two-line text, bare second line, garbage, base64 of other text → exit 1 without echoing the input.
- `tests/ci/releaseSha256Sums.test.ts`: checks the publish-release sparse checkout (`path: src`, `sparse-checkout: scripts`) and `subject-checksums: release-assets/SHA256SUMS`, then runs the `Generate SHA256SUMS` step body in a temp dir laid out like the job (`release-assets/` with fake installers, `.sig` files and `latest.json`; `src/scripts/generate-sha256sums.sh`), verifies with `sha256sum -c SHA256SUMS --ignore-missing` and asserts every line matches `/^[0-9a-f]{64}  [^/]+$/`.
- Each suite also has self-check cases that break a parsed or string copy (inject `if: always()`, ref-based `if:`, `continue-on-error`; strip the base64 check; path-prefixed or `*`-mode checksum lines) and expect the guard to fire.

**Files.** `package.json`, `pnpm-lock.yaml`, `tests/ci/releaseGate.test.ts`, `tests/ci/releaseSigningKeyValidate.test.ts`, `tests/ci/releaseSha256Sums.test.ts`.

**Commits.** `00182e1` (tests + devDependency); this ticket update follows it.

**Verification.**
- `pnpm vitest run tests/ci`: 15 files, 132 tests passed.
- Mutation checks on the real workflow files (backed up and restored, no diff left): `if: always()` added to publish-release → releaseGate fails; base64/first-line `case` removed from the validate step → 5 signing-key tests fail; `continue-on-error: true` on backend plus `if: github.event_name == 'pull_request'` on frontend → releaseGate fails; Generate SHA256SUMS rewritten to emit `./name` paths → releaseSha256Sums fails.
- `pnpm typecheck`: clean. `pnpm format:check`: clean. `pnpm test:unit`: 171 files, 1616 tests passed.
- No end-to-end run on GitHub Actions from this environment; the tests prove the gate's configuration and execute the scripts locally on Linux (the signing-key and SHA256SUMS suites skip when `bash`/`base64`/`sha256sum` are missing).
