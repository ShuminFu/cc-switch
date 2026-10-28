#!/usr/bin/env bash
# Promote a published prerelease to the "latest" release so the Tauri updater
# picks it up. The updater endpoint is releases/latest/download/latest.json,
# and GitHub's releases/latest skips prereleases, so every release built by
# release.yml stays invisible to the updater until it is promoted (CCS-032).
#
# Usage: promote-release.sh <repo> <tag>
#   repo  owner/name, e.g. farion1231/cc-switch
#   tag   stable release tag, e.g. v3.5.0
#
# Requires an authenticated gh (GH_TOKEN with contents: write). The release
# must already have its latest.json asset with a matching version; otherwise
# nothing is changed. See docs/dev/RELEASING.md.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <repo> <tag>" >&2
  exit 2
fi

REPO="$1"
TAG="$2"

# 只允许稳定版标签：带 -beta/-rc 等后缀的版本不应推送给所有用户
if ! [[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "❌ refusing to promote '$TAG': expected a stable tag like v1.2.3" >&2
  exit 1
fi

STATE="$(gh release view "$TAG" --repo "$REPO" --json isDraft,isPrerelease \
  --jq '"\(.isDraft) \(.isPrerelease)"')" || {
  echo "❌ release $TAG not found in $REPO" >&2
  exit 1
}
if [ "${STATE% *}" = "true" ]; then
  echo "❌ release $TAG is still a draft; publish it first" >&2
  exit 1
fi

# latest.json 必须已由 assemble-latest-json 上传且版本一致，否则提升后 updater 会 404 或指向旧版本
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
if ! gh release download "$TAG" --repo "$REPO" --pattern latest.json --dir "$WORK" >/dev/null 2>&1 ||
  [ ! -s "$WORK/latest.json" ]; then
  echo "❌ release $TAG has no latest.json asset; wait for assemble-latest-json" >&2
  exit 1
fi
MANIFEST_VERSION="$(jq -r '.version // empty' "$WORK/latest.json")"
if [ "$MANIFEST_VERSION" != "${TAG#v}" ]; then
  echo "❌ latest.json version '$MANIFEST_VERSION' does not match $TAG" >&2
  exit 1
fi

gh release edit "$TAG" --repo "$REPO" --prerelease=false --latest >/dev/null

LATEST="$(gh api "repos/$REPO/releases/latest" --jq .tag_name)"
if [ "$LATEST" != "$TAG" ]; then
  echo "❌ releases/latest is '$LATEST' after promotion, expected $TAG" >&2
  exit 1
fi

echo "✅ $TAG is now the latest release of $REPO"
