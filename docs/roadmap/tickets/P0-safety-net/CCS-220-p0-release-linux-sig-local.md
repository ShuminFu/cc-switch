# CCS-220: [release] Verify locally that a signed Linux build still produces a valid updater .sig

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:release`

> **Ticket key:** `p0-release-linux-sig-local` · **Phase:** P0 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p0-ci-release-regression-tests`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-029). Source tickets: [CCS-029](../P0-safety-net/CCS-029-p0-ci-release-secrets-env.md).

## Problem
CCS-029's "a fork test tag still produces .sig files on all 3 OSes" needs a real release run, but the Linux leg can be exercised here: `tauri.conf.json` sets `createUpdaterArtifacts: true`, and the release build command is `release.yml` ~327. No local signed build has been recorded since secrets moved to `env:`.

## Proposed change
- Generate a throwaway key: `pnpm tauri signer generate --ci -p testpw -w <scratch>/k.key`.
- Export `TAURI_SIGNING_PRIVATE_KEY="$(cat k.key)"` and the password; run the validate-step script (as in CCS-218) on it.
- Run the Linux build from release.yml (`pnpm tauri build --bundles appimage`, or deb). Repeat once with an unencrypted key and the password unset, to cover the `unset TAURI_SIGNING_PRIVATE_KEY_PASSWORD` branch.
- Record results in CCS-029's Verification section. The pubkey-mismatch warning from the throwaway key is expected.

## Acceptance criteria / verification
- [ ] `src-tauri/target/release/bundle/**/*.sig` exists after both builds.
- [ ] `minisign -V` (or the tauri signer verify path) accepts the .sig against the throwaway pubkey.
- [ ] Results recorded in CCS-029; the macOS/Windows legs stay blocked.

## Dependencies
CCS-218 (`p0-ci-release-regression-tests`).
