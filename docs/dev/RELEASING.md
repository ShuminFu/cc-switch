# Releasing CC Switch

This is the maintainer checklist for cutting a release. The build itself is
automated by [`.github/workflows/release.yml`](../../.github/workflows/release.yml);
making a release visible to the in-app updater is a deliberate manual step.

## Why promotion is needed

- `release.yml` publishes **every** tag as a GitHub **prerelease**.
- The updater endpoint in `src-tauri/tauri.conf.json` is
  `https://github.com/farion1231/cc-switch/releases/latest/download/latest.json`.
- GitHub's `releases/latest` **skips prereleases** (and drafts). So a freshly
  built release is downloadable from its release page, but existing installs
  do not see it until it is promoted to a full, latest release.

This gives maintainers a window to smoke-test the published binaries before
they are pushed to every user.

## 1. Cut the release

1. Bump the version in `package.json`, `src-tauri/Cargo.toml` and
   `src-tauri/tauri.conf.json`, update `CHANGELOG.md` and add release notes
   under `docs/release-notes/`.
   Check the bump locally before tagging; the Release workflow runs the same
   check and stops before building if the tag does not match:

   ```bash
   node scripts/check-versions.mjs vX.Y.Z
   ```

   Add the fix-commit KPI to the release notes: the `unreleased` row of
   `scripts/kpi-fix-commits.sh` (needs tags and full history) counts the
   fix and proxy fix commits since the previous release. Compare it with the
   baseline in [ACCEPTANCE.md](./ACCEPTANCE.md#fix-commit-kpi).

2. Merge to `main`, then tag and push:

   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

3. Wait for the **Release** workflow to finish. It runs CI on the tag, builds
   every platform, publishes the prerelease (`publish-release`) and uploads
   `latest.json` (`assemble-latest-json`). All three must be green.

## 2. Smoke-test the prerelease

Download the artifacts from the release page and check at least one platform
installs and starts. Confirm the release has a `latest.json` asset whose
`version` is `X.Y.Z`.

Every release carries a `SHA256SUMS` asset and a build provenance attestation
for each asset listed in it:

```bash
sha256sum -c SHA256SUMS --ignore-missing
gh attestation verify CC-Switch-vX.Y.Z-Linux-x86_64.AppImage --repo farion1231/cc-switch
```

Attestations need a public repository (or GitHub Enterprise Cloud for a
private one); on a fork, pass the fork as `--repo`.

## 3. Promote to latest

Only stable tags (`vX.Y.Z`, no `-beta`/`-rc` suffix) should be promoted.

**Option A — workflow (preferred).** Actions → **Promote Release** → _Run
workflow_, enter the tag (e.g. `v3.5.0`). The workflow
([`promote-release.yml`](../../.github/workflows/promote-release.yml)) runs
[`scripts/promote-release.sh`](../../scripts/promote-release.sh), which:

- refuses non-stable tags, drafts, and releases without a `latest.json` whose
  `version` matches the tag;
- runs `gh release edit vX.Y.Z --prerelease=false --latest`;
- checks `releases/latest` now reports the tag, then checks the updater URL
  redirects to `releases/download/vX.Y.Z/latest.json`.

If it is started before `assemble-latest-json` has uploaded `latest.json`, the
script refuses and changes nothing; re-run it once the Release workflow is
green. It deliberately does not join `release.yml`'s `release-publish-<tag>`
concurrency group: a newly queued job cancels any job already pending in its
group, which could cancel a pending `assemble-latest-json`.

**Option B — by hand:**

```bash
gh release edit vX.Y.Z --repo farion1231/cc-switch --prerelease=false --latest
# or, with the same safety checks as the workflow:
bash scripts/promote-release.sh farion1231/cc-switch vX.Y.Z
```

## 4. Verify the updater sees it

```bash
curl -sIL https://github.com/farion1231/cc-switch/releases/latest/download/latest.json \
  | grep -i '^location:'
```

The first `Location:` must be
`https://github.com/farion1231/cc-switch/releases/download/vX.Y.Z/latest.json`.
(On a fork, replace `farion1231/cc-switch` with the fork.)

## Rolling back

If a promoted release turns out to be bad, promote the previous good tag
again; `releases/latest` then points back to it and clients stop being offered
the bad version:

```bash
gh release edit vX.Y.(Z-1) --repo farion1231/cc-switch --latest
gh release edit vX.Y.Z --repo farion1231/cc-switch --prerelease
```

Clients that already updated stay on the bad version; ship a fixed `vX.Y.(Z+1)`.
