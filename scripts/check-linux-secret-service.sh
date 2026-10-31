#!/usr/bin/env bash
# Exercise KeyringSecretStore against a real Linux Secret Service (CCS-240).
#
# 1. Starts a private D-Bus session with an unlocked gnome-keyring (isolated
#    keyring directory, never the user's login keyring) and runs the ignored
#    secret_store::keyring_live tests, then checks that no test entries are left.
# 2. Runs probe_fails_without_dbus with no session bus, which must fall back to
#    a 0600 secrets.json.
#
# Requires: dbus-run-session (dbus), gnome-keyring-daemon (gnome-keyring),
# secret-tool (libsecret-tools), cargo.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/src-tauri/Cargo.toml"

for tool in dbus-run-session gnome-keyring-daemon secret-tool cargo; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "missing required tool: $tool (apt install dbus gnome-keyring libsecret-tools)" >&2
    exit 2
  fi
done

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/data" "$WORK/runtime" "$WORK/nobus"
chmod 700 "$WORK/runtime"

SERVICE="cc-switch-test-$$"
export CC_SWITCH_KEYRING_TEST_SERVICE="$SERVICE"
# Build once up front so the D-Bus session is not held open during compilation.
cargo test --manifest-path "$MANIFEST" --lib --no-run

echo "==> Secret Service run (service $SERVICE)"
# XDG_DATA_HOME keeps gnome-keyring's keyring files in the temp dir; the test
# binary only talks to the daemon over D-Bus, so it is unaffected.
XDG_DATA_HOME="$WORK/data" XDG_RUNTIME_DIR="$WORK/runtime" \
  MANIFEST="$MANIFEST" SERVICE="$SERVICE" \
  dbus-run-session -- bash -c '
    set -euo pipefail
    printf pw | gnome-keyring-daemon --unlock --components=secrets >/dev/null
    # Seed an entry with an independent client; shares_entries_with_secret_tool
    # reads it and overwrites it, and the value is checked back with secret-tool.
    export CC_SWITCH_KEYRING_TEST_SEED="seeded-by-secret-tool"
    printf %s "$CC_SWITCH_KEYRING_TEST_SEED" |
      secret-tool store --label="cc-switch test" service "$SERVICE" username test.external
    cargo test --manifest-path "$MANIFEST" --lib secret_store::keyring_live -- \
      --ignored --test-threads=1 --skip probe_fails_without_dbus
    written="$(secret-tool lookup service "$SERVICE" username test.external)"
    if [ "$written" != "written-by-cc-switch" ]; then
      echo "secret-tool lookup returned \"$written\", expected the value written by the test" >&2
      exit 1
    fi
    secret-tool clear service "$SERVICE" username test.external
    leftover="$(secret-tool search --all service "$SERVICE" 2>&1 || true)"
    if [ -n "$leftover" ]; then
      echo "keyring entries left under service $SERVICE:" >&2
      echo "$leftover" >&2
      exit 1
    fi
    echo "no entries left under service $SERVICE"
  '

echo "==> No D-Bus run"
# zbus falls back to $XDG_RUNTIME_DIR/bus (or /run/user/<uid>/bus), so point it at
# an empty directory to simulate a headless session even on a desktop.
env -u DBUS_SESSION_BUS_ADDRESS XDG_RUNTIME_DIR="$WORK/nobus" \
  cargo test --manifest-path "$MANIFEST" --lib \
  secret_store::keyring_live::probe_fails_without_dbus -- --ignored --exact

echo "Linux Secret Service check passed"
