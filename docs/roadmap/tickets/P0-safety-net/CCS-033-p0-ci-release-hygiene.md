# CCS-033: [ci] Add rust-cache, a version consistency check, provenance attestation and SHA256SUMS

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-release-hygiene` · **Phase:** P0 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p0-ci-pin-actions`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
From the CI review ideas: the release job rebuilds from scratch on 5 runners; nothing checks that the tag matches package.json, Cargo.toml, tauri.conf.json and CHANGELOG; releases carry no provenance or checksums.

## Proposed change
- Replace the manual Cargo actions/cache with Swatinem/rust-cache in ci.yml and release.yml.
- Add scripts/check-versions.mjs, run in CI and in the release job, which checks that the tag equals all 3 versions and that a CHANGELOG entry exists.
- Add actions/attest-build-provenance and publish SHA256SUMS.

## Acceptance criteria / verification
- [ ] A mismatched version fails `node scripts/check-versions.mjs v3.18.0`
- [ ] Release assets include SHA256SUMS; `gh attestation verify` passes on a fork

## Dependencies
p0-ci-pin-actions
