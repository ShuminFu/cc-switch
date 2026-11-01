#!/usr/bin/env bash
# Notarize and staple a macOS artifact (release.yml, CI-46 / CI-48).
#
#   scripts/ci/notarize-macos.sh "path/CC Switch.app"   # zips, submits, staples the .app
#   scripts/ci/notarize-macos.sh path/CC-Switch.dmg      # submits and staples the .dmg
#
# Credentials, from the environment (first complete set wins):
#   1. App Store Connect API key (team-owned, revocable; preferred):
#        APPLE_API_KEY_ID, APPLE_API_ISSUER, APPLE_API_KEY_P8 (contents of the .p8)
#   2. Apple ID app-specific password (legacy; the password is passed to
#      notarytool on its command line):
#        APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID
#
# Only submission errors (network, Apple service hiccups) are retried. A
# submission Apple rejects ("Invalid") fails at once and prints Apple's log,
# because resubmitting the same bytes cannot succeed.
set -euo pipefail

target="${1:?usage: notarize-macos.sh <path to .app or .dmg>}"
max_attempts="${NOTARIZE_ATTEMPTS:-3}"
work="$(mktemp -d "${RUNNER_TEMP:-/tmp}/notarize.XXXXXX")"
cleanup() { rm -rf "$work"; }
trap cleanup EXIT

auth=()
if [ -n "${APPLE_API_KEY_ID:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_KEY_P8:-}" ]; then
  (umask 077 && printf '%s\n' "$APPLE_API_KEY_P8" > "$work/AuthKey.p8")
  auth=(--key "$work/AuthKey.p8" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER")
  echo "Notarizing with the App Store Connect API key $APPLE_API_KEY_ID"
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  auth=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
  echo "::warning::Notarizing with an Apple ID app-specific password; switch to an App Store Connect API key (APPLE_API_KEY_ID / APPLE_API_ISSUER / APPLE_API_KEY_P8), see docs/security/key-management.md"
else
  echo "::error::No notarization credentials: set APPLE_API_KEY_ID + APPLE_API_ISSUER + APPLE_API_KEY_P8 (preferred) or APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID" >&2
  exit 1
fi

submit="$target"
if [ -d "$target" ]; then
  submit="$work/$(basename "$target").zip"
  ditto -c -k --sequesterRsrc --keepParent "$target" "$submit"
fi

for attempt in $(seq 1 "$max_attempts"); do
  echo "=== Notarizing $(basename "$target"), attempt ${attempt}/${max_attempts} ==="
  if xcrun notarytool submit "$submit" "${auth[@]}" --wait --output-format json > "$work/result.json"; then
    status="$(jq -r '.status // empty' "$work/result.json")"
    id="$(jq -r '.id // empty' "$work/result.json")"
    echo "Submission $id: ${status:-unknown}"
    if [ "$status" = "Accepted" ]; then
      xcrun stapler staple "$target"
      echo "✅ $(basename "$target") notarized and stapled"
      exit 0
    fi
    if [ -n "$id" ]; then
      xcrun notarytool log "$id" "${auth[@]}" || true
    fi
    echo "::error::Apple rejected $(basename "$target") (status: ${status:-unknown}); not retrying" >&2
    exit 1
  fi
  cat "$work/result.json" 2>/dev/null || true
  if [ "$attempt" -eq "$max_attempts" ]; then
    echo "::error::notarytool submission failed ${max_attempts} times" >&2
    exit 1
  fi
  sleep_seconds=$((attempt * 60))
  echo "⚠️ notarytool submission failed, retrying in ${sleep_seconds}s..."
  sleep "$sleep_seconds"
done
