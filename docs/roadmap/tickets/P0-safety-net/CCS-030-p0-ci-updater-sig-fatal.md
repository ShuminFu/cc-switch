# CCS-030: [ci] Fail the release when an updater artifact or signature is missing and validate latest.json

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p1-high`, `area:ci`

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
- [x] Running the generator script against a dl/ dir missing one .sig exits non-zero
- [x] With all 6 present, `jq -e` passes
- [x] `actionlint` passes

## Dependencies
None.

## Implementation notes
- **Summary:** The Prepare Assets steps in `release.yml` now fail on every platform when an updater artifact or its `.sig` is missing or empty: macOS `.tar.gz.sig`, Windows x64 and ARM64 MSI + `.sig` (both are now `throw`), and Linux AppImage + `.sig`. The inline `echo`-built latest.json generator was replaced by `scripts/generate-latest-json.sh`, which builds the manifest with `jq` and exits non-zero without writing anything unless all 6 keys (`darwin-aarch64`, `darwin-x86_64`, `windows-x86_64`, `windows-aarch64`, `linux-x86_64`, `linux-aarch64`) are present. It also fails when two artifacts match the same key. A final `jq -e` checks that every key has a non-empty `url` and `signature`. `assemble-latest-json` now checks out the repo so the script is available, and the `gh release upload --clobber` step only runs after validation passes.
- **Files:** `.github/workflows/release.yml`, `scripts/generate-latest-json.sh`, `tests/ci/latestJson.test.ts`
- **Commits:** `fc1e0c5` (code + tests)
- **Verification:**
  - `tests/ci/latestJson.test.ts` (15 tests) runs the script on a fake `dl/` dir. With all 6 platforms present it exits 0 and the same `jq -e` assertion passes. A missing `.sig` (each of the 5 updater assets), an empty `.sig`, a missing artifact (each of 5), or a duplicate artifact gives a non-zero exit and no output file. Static checks confirm `release.yml` uses the script and no longer has the warn-only branches. Those regexes match the pre-change `release.yml`, so the test fails before this change and passes after.
  - `pnpm test:unit`: 83 files / 573 tests passed; `pnpm typecheck` clean; `pnpm format:check` clean.
  - YAML parse via `python3 yaml.safe_load`: OK.
  - `actionlint` v1.7.7: passes once its config declares `windows-11-arm` as a known runner label. Without that, the only finding is that this pre-existing label is unknown, because the label is newer than actionlint 1.7.7's built-in list; it is not related to this change. shellcheck is not installed, so actionlint's shellcheck pass did not run.
  - Manual run: `bash scripts/generate-latest-json.sh <dl> o/r v1.0.0 out.json` gives exit 0 with all 6 keys; after deleting the Windows `.sig` it gives exit 1 with "signature missing or empty for CC-Switch-v1.0.0-Windows.msi (windows-x86_64)".
  - Not verified here: an actual tag release on GitHub runners (macOS/Windows/Linux). The PowerShell change was only checked for YAML validity, not executed.
