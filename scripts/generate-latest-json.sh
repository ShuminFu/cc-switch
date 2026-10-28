#!/usr/bin/env bash
# Generate the Tauri updater manifest (latest.json) from downloaded release assets.
#
# Usage: generate-latest-json.sh <assets-dir> <repo> <tag> [output]
#   assets-dir  directory holding the release assets and their .sig files
#   repo        owner/name, used to build the download URLs
#   tag         release tag, e.g. v3.5.0
#   output      manifest path (default: latest.json)
#
# Every updater platform must have an artifact with a non-empty signature;
# otherwise the script exits non-zero and writes nothing, so a release never
# ships a manifest that silently drops a platform (CCS-030).
set -euo pipefail

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
  echo "usage: $0 <assets-dir> <repo> <tag> [output]" >&2
  exit 2
fi

DIR="$1"
REPO="$2"
TAG="$3"
OUT="${4:-latest.json}"

if [ ! -d "$DIR" ]; then
  echo "❌ assets dir not found: $DIR" >&2
  exit 1
fi

VERSION="${TAG#v}"
PUB_DATE="${PUB_DATE:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"
BASE_URL="https://github.com/$REPO/releases/download/$TAG"

# 必须全部存在的 updater 平台键
REQUIRED_KEYS=(
  darwin-aarch64
  darwin-x86_64
  windows-x86_64
  windows-aarch64
  linux-x86_64
  linux-aarch64
)

platforms='{}'
failed=0

# add_platform <key> <artifact-file>
add_platform() {
  local key="$1" file="$2" fname sig
  fname=$(basename "$file")
  if [ "$(jq --arg k "$key" 'has($k)' <<<"$platforms")" = "true" ]; then
    echo "❌ multiple updater artifacts match $key (second: $fname)" >&2
    failed=1
    return
  fi
  if [ ! -s "$file.sig" ]; then
    echo "❌ signature missing or empty for $fname ($key)" >&2
    failed=1
    return
  fi
  # $(...) 去掉末尾换行，与 Tauri 生成的 .sig 内容一致
  sig=$(cat "$file.sig")
  platforms=$(jq --arg k "$key" --arg sig "$sig" --arg url "$BASE_URL/$fname" \
    '. + {($k): {signature: $sig, url: $url}}' <<<"$platforms")
}

shopt -s nullglob
for file in "$DIR"/*; do
  [ -f "$file" ] || continue
  case "$(basename "$file")" in
    *.sig) ;;
    *.tar.gz)
      # macOS universal 包，同时服务 arm64 与 x64
      add_platform darwin-aarch64 "$file"
      add_platform darwin-x86_64 "$file"
      ;;
    *-Windows-arm64.msi) add_platform windows-aarch64 "$file" ;;
    *-Windows.msi) add_platform windows-x86_64 "$file" ;;
    *-Linux-arm64.AppImage | *-Linux-arm64.appimage) add_platform linux-aarch64 "$file" ;;
    *-Linux-x86_64.AppImage | *-Linux-x86_64.appimage) add_platform linux-x86_64 "$file" ;;
  esac
done

for key in "${REQUIRED_KEYS[@]}"; do
  if [ "$(jq --arg k "$key" 'has($k)' <<<"$platforms")" != "true" ]; then
    echo "❌ no updater artifact found for $key" >&2
    failed=1
  fi
done

if [ "$failed" -ne 0 ]; then
  echo "❌ refusing to generate latest.json with missing platforms" >&2
  exit 1
fi

manifest=$(jq -n \
  --arg version "$VERSION" \
  --arg notes "Release $TAG" \
  --arg pub_date "$PUB_DATE" \
  --argjson platforms "$platforms" \
  '{version: $version, notes: $notes, pub_date: $pub_date, platforms: $platforms}')

# 上传前再次断言：6 个键都有非空 url 与 signature
keys_json=$(printf '%s\n' "${REQUIRED_KEYS[@]}" | jq -R . | jq -s .)
jq -e --argjson keys "$keys_json" '
  . as $m
  | ($m.version | type == "string" and length > 0)
    and ($keys | all(. as $k
      | ($m.platforms[$k].url | type == "string" and length > 0)
        and ($m.platforms[$k].signature | type == "string" and length > 0)))
' <<<"$manifest" >/dev/null || {
  echo "❌ latest.json validation failed" >&2
  exit 1
}

printf '%s\n' "$manifest" >"$OUT"
echo "Generated $OUT:"
cat "$OUT"
