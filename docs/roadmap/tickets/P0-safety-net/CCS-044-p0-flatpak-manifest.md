# CCS-044: [packaging] Update the Flatpak manifest and either build it in CI or stop installing flatpak tools

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:packaging`

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
