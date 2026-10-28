# CCS-029: [ci] Pass release secrets via env: and stop exporting signing material to GITHUB_ENV

**Status:** Partial · **Kind:** security · **Labels:** `security`, `phase:P0`, `p1-high`, `area:ci`

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

## Implementation notes

**Summary.** `release.yml` no longer interpolates any `${{ secrets.* }}` into `run:` scripts and no longer writes signing material to `$GITHUB_ENV`.
- The old "Prepare Tauri signing key" step (three-way format detection, re-encoding, export of key and password to `$GITHUB_ENV`) is replaced by **Validate Tauri signing key**: it receives the secret via step `env:` and only checks that it is in the single canonical format — the contents of the `.key` file written by `tauri signer generate` (one-line base64 whose decoded first line is `untrusted comment:`). It derives no new value, prints nothing from the key and writes no key file, so there is nothing to `::add-mask::` or delete afterwards.
- `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` are mapped from secrets directly in the `env:` of the three **Build Tauri App** steps (macOS / Windows / Linux). An empty password is unset before the build so behaviour matches the old "only export when non-empty" logic.
- **Import Apple signing certificate** maps `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` and `KEYCHAIN_PASSWORD` to step `env:` and uses quoted shell variables; the decoded `.p12` is removed via an `EXIT` trap even if a later command fails. (`ORIGINAL_DEFAULT_KEYCHAIN` and `APPLE_SIGNING_IDENTITY` are still passed via `$GITHUB_ENV`; they are not secrets.)
- New regression test `tests/ci/releaseSecrets.test.ts`: every `secrets.*` reference in every workflow must be a `NAME: ${{ secrets.X }}` mapping under `env:`/`with:`; no `GITHUB_ENV` line may mention `TAURI_SIGNING`; in `release.yml` the Tauri key appears only in the validate step + the three build steps and the password only in the three build steps.

**Files.** `.github/workflows/release.yml`, `tests/ci/releaseSecrets.test.ts`, this ticket.

**Commits.** See `git log --grep CCS-029` (code, test and this note land in one commit).

**Verification.**
- `grep -n 'secrets\.' .github/workflows/release.yml` → all matches are `NAME: ${{ secrets.NAME }}` lines under `env:` (none under `run:`).
- `grep -n GITHUB_ENV .github/workflows/release.yml` → only `LIBCLANG_PATH`, `CLANG_PATH`, `ORIGINAL_DEFAULT_KEYCHAIN`, `APPLE_SIGNING_IDENTITY` (and one comment); no `TAURI_SIGNING_*`.
- `python3 -c "import yaml,sys;yaml.safe_load(open(sys.argv[1]))" .github/workflows/release.yml` → OK. `actionlint` → only the pre-existing `windows-11-arm` unknown-runner-label warning (also reported on the previous version; the local actionlint label list is outdated).
- Validate step extracted and run locally: a key produced by `pnpm tauri signer generate --ci -p testpw` passes; empty, raw two-line and garbage values fail with the intended messages. `pnpm tauri signer sign -k "$(cat key)" -p testpw file` produces `file.sig`, confirming the Tauri CLI accepts the raw `.key` contents as the key string (same parsing used for `TAURI_SIGNING_PRIVATE_KEY` in `tauri build`).
- `tests/ci/releaseSecrets.test.ts` against the previous `release.yml`: 3 failed / 10 passed; against the new one: 13 passed.
- `pnpm typecheck` → pass; `pnpm test:unit` → 82 files / 558 tests passed; `pnpm format:check` → clean.

**Remaining (cannot be done in this environment).**
- Acceptance item "A fork test tag still produces .sig files on all 3 OSes" needs real macOS/Windows/Linux runners and the signing secrets; run a test tag on a fork before the next release.
- Human action: confirm the repository's `TAURI_SIGNING_PRIVATE_KEY` secret holds the canonical `.key` file contents (single-line base64, as written by `tauri signer generate`). If it is currently stored as the raw two-line text or the bare second line, re-save it in canonical form — the validate step will now fail fast with a clear message instead of converting it.
- Dependency `p0-ci-pin-actions` (CCS-028) has not landed yet; this change does not touch `uses:` lines, so the two should merge without conflict.
