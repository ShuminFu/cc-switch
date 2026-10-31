# Flatpak Build Guide

This directory contains the Flatpak manifest (`com.ccswitch.desktop`) for CC Switch, used to convert the generated `.deb` artifact into an installable `.flatpak` package via CI or local builds.

## Dependencies

- `flatpak`
- `flatpak-builder`
- Flathub remote (for installing `org.gnome.Platform//46` runtime)

For Ubuntu/Debian:

```bash
sudo apt install flatpak flatpak-builder
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install -y --user flathub org.gnome.Platform//46 org.gnome.Sdk//46
```

## Local Build (Generate .flatpak from .deb)

1) Build the deb on Linux first:

```bash
pnpm tauri build -- --bundles deb
```

2) Copy the generated deb to this directory:

```bash
cp "$(find src-tauri/target/release/bundle -name '*.deb' | head -n 1)" flatpak/cc-switch.deb
```

3) Build the local Flatpak repository and export the `.flatpak`:

```bash
flatpak-builder --force-clean --user --disable-cache --repo flatpak-repo flatpak-build flatpak/com.ccswitch.desktop.yml
flatpak build-bundle --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo flatpak-repo CC-Switch-Linux.flatpak com.ccswitch.desktop
```

4) Install and run:

```bash
flatpak install --user ./CC-Switch-Linux.flatpak
flatpak run com.ccswitch.desktop
```

## Permissions Note

The manifest grants access only to the directories CC Switch manages (`~/.cc-switch`, `~/.claude`, `~/.claude.json`, `~/.codex`, `~/.gemini`, `~/.grok`, `~/.agents`, `~/.openclaw`, `~/.hermes`, `~/.opencode`, `~/.config/opencode`, `~/.config/Claude*`, `~/.local/share/opencode`) plus the shell rc files used by environment-variable conflict detection. Check with:

```bash
flatpak info --show-permissions com.ccswitch.desktop
```

If you use the "directory override" feature with a directory outside that list, grant it explicitly:

```bash
flatpak override --user --filesystem=~/path/to/custom-dir com.ccswitch.desktop
# or restore the previous full-home access:
flatpak override --user --filesystem=home com.ccswitch.desktop
```

Note: Flatpak's `:create` modifier only works with directories, not files. Therefore, `~/.claude.json` cannot use `:create`. If this file doesn't exist on the user's machine, the app may not be able to create it with restricted permissions. Users should either run Claude Code once to generate it, or manually create an empty JSON file (content: `{}`).

If you plan to publish on Flathub or want stricter permission control, adjust the `finish-args` in `flatpak/com.ccswitch.desktop.yml` accordingly.
