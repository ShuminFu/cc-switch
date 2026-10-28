#!/usr/bin/env bash
# Write SHA256SUMS for every release asset in a directory (CCS-033).
#
# Usage: generate-sha256sums.sh <assets-dir>
#
# The file is written to <assets-dir>/SHA256SUMS in `sha256sum` format with
# bare file names, so users can run `sha256sum -c SHA256SUMS` next to the
# downloaded assets. An existing SHA256SUMS is not listed in itself. Exits
# non-zero when the directory holds no assets.
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <assets-dir>" >&2
  exit 2
fi

DIR="$1"
if [ ! -d "$DIR" ]; then
  echo "❌ assets directory not found: $DIR" >&2
  exit 1
fi

cd "$DIR"
FILES=()
while IFS= read -r -d '' f; do
  FILES+=("${f#./}")
done < <(find . -maxdepth 1 -type f ! -name SHA256SUMS ! -name SHA256SUMS.tmp -print0 | LC_ALL=C sort -z)

if [ "${#FILES[@]}" -eq 0 ]; then
  echo "❌ no release assets in $DIR" >&2
  exit 1
fi

sha256sum -- "${FILES[@]}" > SHA256SUMS.tmp
mv SHA256SUMS.tmp SHA256SUMS
sha256sum -c --quiet SHA256SUMS
echo "✅ SHA256SUMS written for ${#FILES[@]} assets"
