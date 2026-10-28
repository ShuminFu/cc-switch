# CCS-044: [packaging] Update the Flatpak manifest and either build it in CI or stop installing flatpak tools

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:packaging`

> **Ticket key:** `p0-flatpak-manifest` · **Phase:** P0 · **Priority:** p3-low · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-flatpak-stale.
- flatpak/com.ccswitch.desktop.yml targets the EOL GNOME 46 runtime and tag-pins three Ayatana sources with no commit.
- The metainfo has no <releases>.
- release.yml installs flatpak and flatpak-builder but never builds. --filesystem=home is a documented choice.

## Evidence
- flatpak/com.ccswitch.desktop.yml; flatpak/com.ccswitch.desktop.metainfo.xml; .github/workflows/release.yml:69-70

## Proposed change
- Bump to a supported GNOME runtime (README too) and add `commit:` pins.
- Generate <releases> from package.json.
- Either add a flatpak-builder + build-bundle CI step, or remove the apt install and fix the "by CI" comment.

## Acceptance criteria / verification
- [ ] `appstreamcli validate --pedantic flatpak/com.ccswitch.desktop.metainfo.xml` is clean
- [ ] `flatpak-builder --force-clean --user --repo repo build flatpak/com.ccswitch.desktop.yml` succeeds, or `grep -rn flatpak .github/` finds nothing

## Dependencies
None.

## Implementation notes

**Summary.** Took the "stop installing flatpak tools" branch: official releases ship no Flatpak (README "Not included in official releases"), so `release.yml` no longer apt-installs `flatpak`/`flatpak-builder`, and the "by CI" wording in the manifest and `flatpak/README.md` now says the `.deb` is copied in by hand for a local build. The manifest moves from the EOL GNOME 46 runtime to `org.gnome.Platform//50` (README updated to match) and the three Ayatana git sources keep their tags plus `commit:` pins (peeled tag commits from `git ls-remote`). The metainfo gains `<developer>`, `<content_rating type="oars-1.1"/>` and a `<releases>` block generated from the newest 10 dated `CHANGELOG.md` entries by the new `scripts/sync-metainfo-releases.mjs`; it refuses a non-prerelease `package.json` version with no dated entry, and CI (`ci.yml`) runs it with `--check` so a version bump that skips regeneration fails. `--filesystem=home` stays as the documented choice.

**Files.** `.github/workflows/release.yml`, `.github/workflows/ci.yml`, `flatpak/com.ccswitch.desktop.yml`, `flatpak/com.ccswitch.desktop.metainfo.xml`, `flatpak/README.md`, `scripts/sync-metainfo-releases.mjs` (+ `.d.mts`), `tests/scripts/syncMetainfoReleases.test.ts`.

**Commits.** `b6c6145` (code), plus the follow-up commit recording these notes.

**Verification.**
- `appstreamcli validate --pedantic --no-net flatpak/com.ccswitch.desktop.metainfo.xml` → "Validation was successful." (before: `releases-info-missing` pedantic, `content-rating-missing`, `developer-info-missing`). Without `--no-net` the only output is `url-not-reachable` (HTTP 403) for the homepage/bugtracker URLs, which the sandbox egress proxy causes.
- `grep -rn flatpak .github/` → no matches (exit 1).
- `node scripts/sync-metainfo-releases.mjs --check` → up to date (3.17.0).
- `tests/scripts/syncMetainfoReleases.test.ts` 9/9 pass; the manifest checks (runtime ≥ 50 and matches README, every git source has a 40-hex `commit:`, release.yml has no `flatpak`) fail on the previous tree.
- `pnpm typecheck` OK; `pnpm format:check` OK; `pnpm test:unit` 92 files / 655 tests pass; ci.yml, release.yml and the manifest parse with `yaml.safe_load` (actionlint not installed).

**Not verified locally.** `flatpak-builder` against GNOME 50 was not run: flathub.org is unreachable from this sandbox and flatpak is not installed. That is no longer required for acceptance (the grep branch holds), but the first person to build locally should confirm GNOME 50 still ships `webkit2gtk-4.1`, which the Tauri `.deb` links against.

**Review.** Re-checked independently: the three `commit:` pins match the peeled tags from `git ls-remote`; `appstreamcli validate --pedantic --no-net` is clean; `grep -rn flatpak .github/` finds nothing; `--check` reports up to date; the new tests (9/9), `pnpm test:unit` (655), `pnpm typecheck` and `pnpm format:check` pass. One gap fixed: `docs/dev/RELEASING.md` listed the version-bump steps without the metainfo regeneration, so a release bump PR that followed it would fail the new CI check. It now says to run `node scripts/sync-metainfo-releases.mjs`. Whether GNOME 50 still ships `webkit2gtk-4.1` has still not been checked with a real `flatpak-builder` run.
