# CCS-030: [ci] Fail the release when an updater artifact or signature is missing and validate latest.json

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p1-high`, `area:ci`

> **Ticket key:** `p0-ci-updater-sig-fatal` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-missing-sig-not-fatal. On macOS, Windows x64 and Linux, a missing .sig, MSI or AppImage only produces a warning. latest.json silently drops the platform, so after promotion those users stop getting auto-updates.

## Evidence
- release.yml:314 (macOS), 461-475 (Win x64 warn), 525-532 (Linux), 669-723 (generator), 728 upload --clobber

## Proposed change
- Make missing artifacts fatal (`exit 1`/`throw`) on every platform.
- Generate latest.json with jq.
- Before uploading, assert all 6 keys (darwin-aarch64/x86_64, windows-x86_64/aarch64, linux-x86_64/aarch64) have a non-empty url and signature.

## Acceptance criteria / verification
- [ ] Running the generator script against a dl/ dir missing one .sig exits non-zero
- [ ] With all 6 present, `jq -e` passes
- [ ] `actionlint` passes

## Dependencies
None.
