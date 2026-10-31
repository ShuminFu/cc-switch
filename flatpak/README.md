# Flatpak Build Guide

This directory contains the Flatpak manifest (`com.ccswitch.desktop`) for CC Switch, used to convert the generated `.deb` artifact into an installable `.flatpak` package via CI or local builds.

## Status

The Flatpak is **community-maintained and not part of the official release
assets** (CI-23). `.github/workflows/flatpak.yml` builds it monthly (and on
demand) from a freshly built `.deb` so the manifest does not rot, and uploads the
bundle as a workflow artifact.

Known limitations inside the sandbox:

- **Host CLIs and terminals.** CLI version detection and "open in terminal" run
  host programs directly, which the sandbox hides (`/usr/bin` is the GNOME
  runtime). The manifest already grants `--talk-name=org.freedesktop.Flatpak`
  so the app can switch to `flatpak-spawn --host`, but the app does not do that
  yet (CI-44), so those two features report "not installed" / "no terminal".
- **Deep links** (`ccswitch://`) work: the desktop file declares
  `MimeType=x-scheme-handler/ccswitch;` and passes the URL with `%U`.

## Dependencies

- `flatpak`
- `flatpak-builder`
- Flathub remote (for installing `org.gnome.Platform//48` runtime)

For Ubuntu/Debian:

```bash
sudo apt install flatpak flatpak-builder
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install -y --user flathub org.gnome.Platform//48 org.gnome.Sdk//48
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

The current manifest uses `--filesystem=home` by default for "download and run" convenience, allowing the app to directly read/write CLI configuration files and app data on the host (and supporting the "directory override" feature).

If you prefer minimal permissions (e.g., for Flathub submission or security concerns), you can replace `--filesystem=home` in `flatpak/com.ccswitch.desktop.yml` with more precise grants:

```yaml
  - --filesystem=~/.cc-switch:create
  - --filesystem=~/.claude:create
  - --filesystem=~/.claude.json
  - --filesystem=~/.codex:create
  - --filesystem=~/.gemini:create
  - --filesystem=~/.config/opencode:create
  - --filesystem=~/.openclaw:create
```

Note: Flatpak's `:create` modifier only works with directories, not files. Therefore, `~/.claude.json` cannot use `:create`. If this file doesn't exist on the user's machine, the app may not be able to create it with restricted permissions. Users should either run Claude Code once to generate it, or manually create an empty JSON file (content: `{}`).

If you plan to publish on Flathub or want stricter permission control, adjust the `finish-args` in `flatpak/com.ccswitch.desktop.yml` accordingly.
