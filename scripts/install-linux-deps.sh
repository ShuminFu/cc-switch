#!/usr/bin/env bash
# Install the system packages needed to build and test CC Switch on Debian /
# Ubuntu (22.04+). Used by CI (.github/actions/linux-deps), the release
# workflow, the devcontainer and contributors.
#
#   scripts/install-linux-deps.sh            # build + test deps
#   scripts/install-linux-deps.sh --bundle   # + packaging tools for `pnpm tauri build`
#
# Runs apt-get through sudo when not already root.
set -euo pipefail

bundle=false
for arg in "$@"; do
  case "$arg" in
    --bundle) bundle=true ;;
    -h | --help)
      sed -n '2,8p' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

if [ "$(id -u)" -eq 0 ]; then
  SUDO=""
else
  SUDO="sudo"
fi
apt_install() {
  $SUDO apt-get install -y --no-install-recommends "$@"
}

export DEBIAN_FRONTEND=noninteractive
$SUDO apt-get update

# Compiler toolchain, pkg-config and the GTK/GLib stack (gdk-3.0, glib-2.0, gio-2.0).
apt_install build-essential pkg-config curl file libssl-dev \
  libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev

# WebKit2GTK and libsoup changed major versions across Ubuntu images: prefer the
# Tauri 2 versions (4.1 / 3.0) and fall back for older images.
apt_install libwebkit2gtk-4.1-dev || apt_install libwebkit2gtk-4.0-dev
apt_install libsoup-3.0-dev || apt_install libsoup2.4-dev

if [ "$bundle" = true ]; then
  # AppImage (patchelf, file), .rpm and .deb bundling, desktop integration.
  apt_install wget patchelf rpm elfutils xdg-utils
fi
