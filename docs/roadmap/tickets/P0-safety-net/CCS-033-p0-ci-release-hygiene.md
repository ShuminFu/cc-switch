# CCS-033: [ci] Add rust-cache, a version consistency check, provenance attestation and SHA256SUMS

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-release-hygiene` · **Phase:** P0 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p0-ci-pin-actions`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
From the CI review ideas: the release job rebuilds from scratch on 5 runners; nothing checks that the tag matches package.json, Cargo.toml, tauri.conf.json and CHANGELOG; releases carry no provenance or checksums.

## Proposed change
- Replace the manual Cargo actions/cache with Swatinem/rust-cache in ci.yml and release.yml.
- Add scripts/check-versions.mjs, run in CI and in the release job, which checks that the tag equals all 3 versions and that a CHANGELOG entry exists.
- Add actions/attest-build-provenance and publish SHA256SUMS.

## Acceptance criteria / verification
- [x] A mismatched version fails `node scripts/check-versions.mjs v3.18.0`
- [ ] Release assets include SHA256SUMS; `gh attestation verify` passes on a fork

## Dependencies
p0-ci-pin-actions

## Implementation notes

**Summary.**
- `ci.yml`: the four manual Cargo `actions/cache` steps (backend, msrv,
  tauri-build-smoke, coverage-rust) are replaced with `Swatinem/rust-cache@v2`
  (`workspaces: src-tauri -> target`, one `key` per job / matrix OS). The
  frontend job runs `node scripts/check-versions.mjs`; when release.yml calls
  ci.yml on a tag, `GITHUB_REF` is `refs/tags/*` and the tag is checked too.
- `release.yml`: the build matrix gains `Swatinem/rust-cache@v2`
  (`key: release-${{ matrix.os }}`, `cache-on-failure: true` so a re-run of the
  same tag after a flaky notarization/upload step reuses the build). A new
  `check-versions` job runs `node scripts/check-versions.mjs "$GITHUB_REF_NAME"`
  and `release` now `needs: [ci, check-versions]`, so a mismatch fails before
  the five runners start. `publish-release` gets `id-token: write` and
  `attestations: write`, writes `release-assets/SHA256SUMS` with
  `scripts/generate-sha256sums.sh` (sparse checkout of `scripts/`), then runs
  `actions/attest-build-provenance@v3` with
  `subject-checksums: release-assets/SHA256SUMS` before the upload; SHA256SUMS
  is uploaded with the other assets and the release body shows how to verify.
- `scripts/check-versions.mjs` (+ `.d.mts`): checks package.json,
  `[package].version` in Cargo.toml and tauri.conf.json agree, that CHANGELOG.md
  has `## [<version>]` (a prerelease version may use `## [Unreleased]`), and
  that the tag (argument or `GITHUB_REF`) equals `v<version>`. `--root` points
  it at another tree (used by tests).
- `docs/dev/RELEASING.md`: run the check before tagging; how to verify
  SHA256SUMS and attestations.

**Files.** `.github/workflows/ci.yml`, `.github/workflows/release.yml`,
`scripts/check-versions.mjs`, `scripts/check-versions.d.mts`,
`scripts/generate-sha256sums.sh`, `tests/scripts/checkVersions.test.ts`,
`tests/ci/releaseHygiene.test.ts`, `docs/dev/RELEASING.md`.

**Commits.** `3736bef` (implementation); this ticket update follows in a
separate commit.

**Verification.**
- `node scripts/check-versions.mjs v3.18.0` → exit 1 (tag does not match the
  3.17.0 in all three manifests); `node scripts/check-versions.mjs v3.17.0` and
  with no argument → exit 0.
- `npx vitest run tests/ci tests/scripts` → 9 files, 91 tests passed (new:
  checkVersions 11, releaseHygiene 11). The workflow assertions fail on the
  previous workflows (no rust-cache, no check-versions job, no SHA256SUMS /
  attestation step).
- `pnpm test:unit` → 87 files, 618 tests passed; `pnpm typecheck` clean;
  `pnpm format:check` clean; `bash -n scripts/generate-sha256sums.sh` ok.
- YAML: `python3 -c "import yaml,sys;yaml.safe_load(open(sys.argv[1]))"` passes
  for ci.yml and release.yml. actionlint and shellcheck are not installed here.
- actionlint v1.7.12 with shellcheck 0.11.0 on PATH (CCS-217): `actionlint .github/workflows/*.yml` → exit 0 with the committed `.github/actionlint.yaml`
  (after quoting `$GITHUB_OUTPUT` in the pnpm-store steps, SC2086);
  `shellcheck scripts/generate-sha256sums.sh` → exit 0, no findings.

**Remaining (Partial).**
- "Release assets include SHA256SUMS; `gh attestation verify` passes on a
  fork" needs a real tag run on GitHub (secrets, macOS/Windows runners). After
  the next release (or a fork release), run
  `sha256sum -c SHA256SUMS --ignore-missing` and
  `gh attestation verify <asset> --repo <owner>/<repo>`. Attestations require
  a public repository (or GitHub Enterprise Cloud for a private fork); on a
  private fork without it the attest step fails and blocks publishing.
- Done in CCS-028 (p0-ci-pin-actions): both new actions are pinned to
  commit SHAs. `Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3`
  at `ci.yml` lines 130, 208, 276 and 413 and `release.yml` line 118;
  `actions/attest-build-provenance@977bb373ede98d70efdf65b84cb5f73e068dcc2a # v3.0.0`
  at `release.yml` line 738 (line numbers at f13a403).

### Remaining after triage

- Blocked: AC2: SHA256SUMS in the release assets and `gh attestation verify` passing. Needs a signed tag run on a public fork (OIDC provenance cannot be produced locally).
- Done in [CCS-217](../P0-safety-net/CCS-217-p0-ci-actionlint-shellcheck.md): actionlint with shellcheck passes and its config is committed.
- Follow-up [CCS-218](../P0-safety-net/CCS-218-p0-ci-release-regression-tests.md): Add executable regression tests for the release gate, signing-key validation and SHA256SUMS.
- Follow-up [CCS-221](../P0-safety-net/CCS-221-p0-docs-ticket-evidence-refresh.md): Tick already-verified acceptance boxes and refresh stale evidence in Partial tickets.

### Evidence refresh (CCS-221)

Re-run on the lane2/followups tree at f13a403:
- `node scripts/check-versions.mjs v3.18.0` → exit 1 (tag does not match
  3.17.0 in package.json, Cargo.toml and tauri.conf.json);
  `node scripts/check-versions.mjs` → exit 0 ("version 3.17.0 is consistent").
  AC1 ticked.
- `npx vitest run tests/ci/releaseHygiene.test.ts tests/scripts/checkVersions.test.ts`
  → 2 files, 22 tests passed.
- `grep -n "rust-cache\|attest-build-provenance" .github/workflows/*.yml`
  confirms the SHA pins listed above; no tag-only reference is left.
- AC2 stays open (Blocked, needs a signed tag run on a public fork).
