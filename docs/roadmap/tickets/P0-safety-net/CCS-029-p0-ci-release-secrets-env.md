# CCS-029: [ci] Pass release secrets via env: and stop exporting signing material to GITHUB_ENV

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P0`, `p1-high`, `area:ci`

> **Ticket key:** `p0-ci-release-secrets-env` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S · **Depends on:** `p0-ci-pin-actions`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-secrets-inline-run. Secrets are interpolated into run: scripts. Both TAURI_SIGNING_PRIVATE_KEY (re-encoded, so log masking may miss it) and its password are written to $GITHUB_ENV, which exposes them to every later step.

## Evidence
- .github/workflows/release.yml:137,143,183-185,196,204,207,212,215

## Proposed change
- Map each secret to step env and use quoted shell variables.
- Set TAURI_SIGNING_PRIVATE_KEY/_PASSWORD only in the env of the three Build Tauri App steps.
- Settle on one canonical key format and drop the format detection (CI idea 8). `::add-mask::` any derived value, and delete the key file after the build.

## Acceptance criteria / verification
- [ ] `grep -n 'secrets\.' release.yml` shows matches only inside env:/with:
- [ ] `grep -n GITHUB_ENV release.yml` shows no TAURI_SIGNING_*
- [ ] A fork test tag still produces .sig files on all 3 OSes

## Dependencies
p0-ci-pin-actions (same file; land in sequence).
