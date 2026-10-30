# CCS-220: [release] Verify locally that a signed Linux build still produces a valid updater .sig

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:release`

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
- [x] `src-tauri/target/release/bundle/**/*.sig` exists after both builds.
- [x] `minisign -V` (or the tauri signer verify path) accepts the .sig against the throwaway pubkey.
- [x] Results recorded in CCS-029; the macOS/Windows legs stay blocked.

## Dependencies
CCS-218 (`p0-ci-release-regression-tests`).

## Implementation notes

**Summary.** This ticket only needed verification; no code or workflow changed. Two signed Linux release builds were run at `845ff96` with throwaway keys, using the `Validate Tauri signing key` and `Build Tauri App (Linux)` scripts taken from `release.yml` without changes. One build used an encrypted key with `TAURI_SIGNING_PRIVATE_KEY_PASSWORD=testpw`. The other used a `tauri signer generate` key with an empty password and an empty `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, which takes the `unset` branch. Both builds produced valid updater signatures for the AppImage, the .deb and the .rpm. The full results are in [CCS-029](CCS-029-p0-ci-release-secrets-env.md) under "Linux leg verified locally (CCS-220)".

**Files.** `docs/roadmap/tickets/P0-safety-net/CCS-029-p0-ci-release-secrets-env.md` (results; the blocked item now points to the Linux evidence) and this ticket.

**Commits.** See `git log --grep CCS-220` (one docs commit).

**Verification.**
- Steps were extracted with `python3 -I extract_step.py .github/workflows/release.yml "<step name>"` (`yaml.safe_load`, then the step's `run:`) and run with `bash`. Environment: `CI=true`, tauri-cli 2.8.1 (`pnpm tauri`), `tauri` crate 2.10.3, release profile. The first build compiled from cold in 16m07s; the second rebuilt only `cc-switch`, in 9m43s.
- Build 1 (encrypted key and password): exit 0. `Finished 3 updater signatures` at `src-tauri/target/release/bundle/{appimage,deb,rpm}/*.sig`.
- Build 2 (empty-password key, password unset): the trace shows `+ unset TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; exit 0; same 3 `.sig` files, regenerated after `bundle/` was deleted (the sha256 values differ from build 1).
- `minisign -V` (0.11, base64-decoded `.key.pub`/`.sig`) printed `Signature and comment signature verified` for 3/3 artifacts in both builds. With the wrong pubkey it reported a key id mismatch. Once the `.deb` was modified it printed `Signature verification failed`.
- The pubkey-mismatch warning against `plugins.updater.pubkey` appeared as expected.

**Observations (no change made here).**
- Each bundle printed `Failed to add bundler type to the binary: __TAURI_BUNDLE_TYPE variable not found in binary`. The installed `@tauri-apps/cli` 2.8.1 is older than the `tauri` crate 2.10.3 in `Cargo.lock`, so the CLI does not patch the bundle-type marker. Signing is not affected. `latest.json` lists only Linux AppImages, and the updater identifies those by the `APPIMAGE` env var, so in-app updates are probably fine; a deb/rpm install would not report its bundle type. To fix it, bump `@tauri-apps/cli` to the crate's minor version (≥ 2.10) and add a test that fails when the CLI and crate minor versions differ. This should be its own ticket.
- `pnpm tauri signer sign` with no `-p` and no password env tries to read a password from a TTY. It fails in a non-interactive shell (`os error 6`), even for an empty-password key. `tauri build` with the password unset signs fine, so `release.yml` is unaffected; only someone signing by hand with the CLI would hit this. A KDF-less minisign key (`minisign -G -W`, wrapped in base64) passes the validate step's format check, but `tauri signer sign` cannot use it. Every key from `tauri signer generate` uses scrypt, so this only matters for hand-made keys.

**Remaining.** The macOS and Windows legs of CCS-029's "fork test tag produces .sig on all 3 OSes" are still blocked. They need a real `release.yml` run on macOS/Windows runners with the signing secrets.

**Cleanup.** The 2.7 GB `src-tauri/target/release` dir created for these builds and the throwaway keys were deleted afterwards. An old `src-tauri/target/x86_64-pc-windows-gnu` cross-build cache was also deleted to free disk space.

