#!/usr/bin/env bash
# Count fix commits per release, the KPI from the P0 process rules (CCS-034).
#
# Usage: kpi-fix-commits.sh [-C <repo>] [--ref <ref>] [<releases>]
#
# Takes the last <releases> (default 3) stable tags vX.Y.Z reachable from
# <ref> (default HEAD) and prints one Markdown table row per release window
# <previous tag>..<tag>, plus an "unreleased" row for <last tag>..<ref> when
# <ref> is ahead of the last tag. Merge commits are not counted.
#
# - fix commit:       subject starts with `fix:` / `fix(scope):` / `fix!:`,
#                     `revert:` / `revert(scope):` or `Revert "`
# - proxy fix commit: a fix commit that touches src-tauri/src/proxy/
#
# Needs the release tags and full history (`git fetch --tags --unshallow`, or
# point -C at a full clone). Exits 1 when fewer than <releases> + 1 tags are
# reachable, because the first window would have no lower bound.
set -euo pipefail

REPO="."
REF="HEAD"
COUNT=3
PROXY_PATH="src-tauri/src/proxy/"
FIX_RE='^(fix|revert)(\([^)]*\))?!?:|^Revert "'

usage() {
  echo "usage: $0 [-C <repo>] [--ref <ref>] [<releases>]" >&2
  exit 2
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -C)
      [ "$#" -ge 2 ] || usage
      REPO="$2"
      shift 2
      ;;
    --ref)
      [ "$#" -ge 2 ] || usage
      REF="$2"
      shift 2
      ;;
    -h | --help) usage ;;
    *)
      case "$1" in
        '' | *[!0-9]*) usage ;;
      esac
      COUNT="$1"
      shift
      ;;
  esac
done
[ "$COUNT" -ge 1 ] || usage

g() { git -C "$REPO" "$@"; }

g rev-parse --verify --quiet "$REF^{commit}" >/dev/null || {
  echo "❌ unknown ref: $REF" >&2
  exit 1
}

TAGS=()
while IFS= read -r t; do
  [ -n "$t" ] && TAGS+=("$t")
done < <(g tag --merged "$REF" --sort=v:refname --list 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' || true)

NEED=$((COUNT + 1))
if [ "${#TAGS[@]}" -lt "$NEED" ]; then
  echo "❌ need $NEED release tags reachable from $REF, found ${#TAGS[@]}" >&2
  echo "   fetch them with: git fetch --tags --unshallow" >&2
  exit 1
fi

# Prints "<commits> <fix commits> <proxy fix commits>" for a revision range.
count_range() {
  local range="$1" total fixes proxy
  total=$(g rev-list --count --no-merges "$range")
  fixes=$(g log --no-merges --format=%s "$range" | grep -cE "$FIX_RE" || true)
  proxy=$(g log --no-merges --format=%s "$range" -- "$PROXY_PATH" | grep -cE "$FIX_RE" || true)
  echo "$total $fixes $proxy"
}

row() {
  local label="$1" range="$2" total fixes proxy pct
  read -r total fixes proxy < <(count_range "$range")
  if [ "$total" -gt 0 ]; then
    pct=$((fixes * 100 / total))
  else
    pct=0
  fi
  echo "| $label | \`$range\` | $total | $fixes | $pct% | $proxy |"
}

echo "| Release | Range | Commits | Fix commits | Fix % | Proxy fix commits |"
echo "|---------|-------|--------:|------------:|------:|------------------:|"
START=$((${#TAGS[@]} - COUNT))
for ((i = START; i < ${#TAGS[@]}; i++)); do
  row "${TAGS[i]}" "${TAGS[i - 1]}..${TAGS[i]}"
done

LAST="${TAGS[${#TAGS[@]} - 1]}"
if [ "$(g rev-parse "$LAST^{commit}")" != "$(g rev-parse "$REF^{commit}")" ]; then
  row "unreleased" "$LAST..$REF"
fi
