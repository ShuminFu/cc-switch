# Releasing CC Switch

How to cut a release (CI-28). Written so a second maintainer can ship a patch release with
nothing but this page. The automation lives in `.github/workflows/release.yml`; signing keys
are described in [docs/security/key-management.md](docs/security/key-management.md).

## What the pipeline does

Pushing a `vX.Y.Z` tag runs, in order:

1. **preflight**: refuses tags that are not on `main`; tags with a hyphen (`v3.18.0-rc.1`) become
   prereleases, all others full releases.
2. **verify**: the whole CI workflow (`ci.yml`) on the tagged commit, including the
   version/tag consistency check.
3. **release** (5 legs: Windows x64/ARM64, Linux x64/ARM64, macOS Universal): build, sign
   (updater minisign key; macOS Developer ID + notarization; Windows Authenticode when the
   certificate secret is configured), smoke-test the `.deb`, upload the assets.
4. **assemble-latest-json**: builds `latest.json` with the updater notes from your release notes,
   verifies every signature against the pubkey in `tauri.conf.json`, renders the GitHub release
   body from `docs/release-notes/vX.Y.Z-en.md`, and produces the SBOM and `SHA256SUMS`.
5. **publish-release**: attests provenance for every file, creates a draft release with all
   assets, checks every `latest.json` URL is an asset, publishes it (as latest, or as a
   prerelease), and polls the updater endpoint until it serves the new version.

**Rehearse first**: Actions → Release → Run workflow on `main` (`dry_run: true`, optionally
`sign: true`) runs everything except publishing.

## Checklist

### 1. Preconditions

- [ ] `main` is green (CI, including the ARM64 jobs).
- [ ] No open issue labelled `release-blocker`.
- [ ] Toolchain pins reviewed (monthly at most): `rust-toolchain.toml`, `.node-version`,
      `packageManager` in `package.json`, and the runner images in the workflows. Dependabot does
      not update these.

### 2. Changelog and notes

- [ ] Draft the changelog section: `git cliff --unreleased --tag vX.Y.Z` (config in `cliff.toml`),
      edit it into user-facing language and put it under `## [X.Y.Z] - YYYY-MM-DD` in
      `CHANGELOG.md`, leaving an empty `## [Unreleased]` above it; add the compare link at the
      bottom.
- [ ] Write `docs/release-notes/vX.Y.Z-en.md`, `-zh.md` and `-ja.md` (copy the structure of the
      previous release: summary blockquote, language links, `## Highlights`, Added / Changed /
      Fixed, Upgrade Notes). **The release fails without the `-en.md` file.** The summary and
      `## Highlights` become the GitHub release body; `## Highlights` (plain text, about 1.5 KB)
      becomes the in-app updater notes. Preview both with
      `pnpm release:notes --tag vX.Y.Z --body /tmp/body.md --updater-notes /tmp/notes.txt`.
- [ ] Add the version to `docs/release-notes/README.md`.
- [ ] Flatpak (community): add a `<release>` entry to `flatpak/com.ccswitch.desktop.metainfo.xml`.

### 3. Version bump

```bash
pnpm release:bump X.Y.Z            # package.json, Cargo.toml, Cargo.lock, tauri.conf.json
node scripts/bump-version.mjs --check --tag vX.Y.Z
git commit -am "chore(release): vX.Y.Z"
```

Open this as a PR (`chore(release): vX.Y.Z`), let CI pass, merge it.

### 4. Tag

```bash
git switch main && git pull
git tag -s vX.Y.Z -m "CC Switch vX.Y.Z"   # -s if you sign tags; -a otherwise
git push origin vX.Y.Z
```

Approve the `release` environment when GitHub asks. A full run takes roughly 45 to 75 minutes,
dominated by the macOS universal build and notarization.

### 5. After publishing

- [ ] Open the release page: assets, `SHA256SUMS`, SBOM and body look right.
- [ ] `curl -sL https://github.com/farion1231/cc-switch/releases/latest/download/latest.json | jq .version`
      shows the new version (the workflow already checks this).
- [ ] An installed older build offers the update (Settings → About → Check for updates).
- [ ] Package channels. None of them is automated yet (CI-22); each needs its maintainer's
      account:
  - Homebrew cask `cc-switch`: bump with `brew bump-cask-pr --version X.Y.Z cc-switch`, or wait
    for Homebrew's autobump.
  - AUR `cc-switch-bin`: maintained outside this repository; the `SHA256SUMS` asset has the
    hashes the PKGBUILD needs.
  - No winget/Scoop/Nix packages exist.
- [ ] Announce (website, release discussion).

## Prereleases

Tag `vX.Y.Z-rc.N` (or `-beta.N`). It is published as a GitHub prerelease, which the in-app
updater ignores, so only people who download it manually get it. Release notes are still
required (`docs/release-notes/vX.Y.Z-rc.N-en.md`).

## Rollback

The updater follows `releases/latest`, so:

1. **Stop the bad release from spreading**: mark it as a prerelease
   (`gh release edit vX.Y.Z --prerelease --latest=false`) and mark the previous good release as
   latest (`gh release edit vX.Y.W --latest`). Clients that have not updated stop seeing it.
2. **Fix forward**: users who already installed the bad version only move on through a newer
   version (the updater never downgrades). Ship `vX.Y.(Z+1)` with the fix or a revert, following
   this checklist.
3. Never delete or re-point a published tag: packagers and checksums depend on it. If a tag push
   failed before publishing anything, delete the tag (`git push origin :refs/tags/vX.Y.Z`), fix,
   and tag again.
4. To re-run publishing for an existing tag (for example after a transient GitHub failure), run
   the Release workflow on that tag with `dry_run: false`.

## Troubleshooting the pipeline

| Symptom | Cause / fix |
| --- | --- |
| `Tag … is not reachable from origin/main` | Tag the merge commit on `main`, not a branch |
| Version consistency check fails | Run `pnpm release:bump X.Y.Z` and commit all four files |
| `docs/release-notes/vX.Y.Z-en.md is missing` | Write the notes (step 2), push, tag again |
| macOS: `Apple rejected …` | Notarization log is printed above; usually an unsigned nested binary or a missing entitlement. Not retried |
| macOS: notarytool submission failed 3 times | Apple service problem; re-run the failed job |
| `latest.json failed structural validation` / signature does not verify | Updater key secret is wrong or rotated without a pubkey change; see key-management.md |
| Windows: `Authenticode signature … is NotSigned` | The `WINDOWS_CERTIFICATE` secret is set but Tauri did not sign; check the certificate import step |
