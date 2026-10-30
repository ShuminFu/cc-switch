#!/usr/bin/env bash
# Regenerate a migration-harness DB fixture from a released tag or commit.
#
# Usage: gen-db-fixture.sh <git-ref> [output.sql]
#   git-ref     tag or commit whose app created the database, e.g. v3.12.3
#   output.sql  dump path (default: src-tauri/tests/fixtures/db/v<N>.sql,
#               where N is SCHEMA_VERSION at <git-ref>)
#
# How it works (CCS-011):
#   1. Check <git-ref> out into a temporary git worktree.
#   2. Append a one-off #[test] to src-tauri/src/database/mod.rs at that ref that
#      calls the old app's own `Database::init()` with HOME pointed at a temp dir,
#      so the schema, seed rows and user_version are exactly what that release
#      wrote for a fresh install. The test is built with the old Cargo.lock.
#   3. Insert a fixed set of sample user rows (providers, endpoints, MCP,
#      prompts, skills, settings, usage logs, ...). Only columns that exist in
#      that schema are written, so the same sample works for every version.
#   4. Dump the result as SQL (python3 sqlite3.iterdump) plus PRAGMA user_version.
#      The seeded model_pricing rows are left out to keep fixtures small; the
#      current app seeds and repairs pricing on every start.
#
# The ref must be reachable from the git repo the script runs in. CI and local
# clones need full history and tags (actions/checkout `fetch-depth: 0`, or
# `git fetch --unshallow --tags`). Release tags live upstream:
#   git fetch --tags https://github.com/farion1231/cc-switch
# Set CCS_FIXTURE_GIT_DIR to use another clone that has the refs.
#
# Build cost: the old crate is compiled once per ref. CARGO_TARGET_DIR defaults
# to src-tauri/target/fixture-gen so dependencies are shared across refs; set
# it to src-tauri/target to also reuse the main build cache. The toolchain
# defaults to the one in rust-toolchain.toml (override with RUSTUP_TOOLCHAIN).
# Set CCS_FIXTURE_LOCKFILE=current to build the old crate against this
# checkout's src-tauri/Cargo.lock instead of the ref's own lockfile: far fewer
# dependencies get rebuilt (the schema comes from the old crate's code, not
# from dependency versions), at the cost of not using the exact released deps.
#
# Requires: git, cargo, python3.
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $0 <git-ref> [output.sql]" >&2
  exit 2
fi

REF="$1"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GIT_SRC="${CCS_FIXTURE_GIT_DIR:-$ROOT}"
SHA="$(git -C "$GIT_SRC" rev-parse --verify "$REF^{commit}")"

TOOLCHAIN_DEFAULT="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$ROOT/rust-toolchain.toml" 2>/dev/null || true)"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-$TOOLCHAIN_DEFAULT}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/src-tauri/target/fixture-gen}"
export CARGO_INCREMENTAL=0

WORK="$(mktemp -d "${TMPDIR:-/tmp}/ccs-db-fixture.XXXXXX")"
WT="$WORK/src"
cleanup() {
  git -C "$GIT_SRC" worktree remove --force "$WT" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

git -C "$GIT_SRC" worktree add --detach "$WT" "$SHA" >/dev/null

MOD_RS="$WT/src-tauri/src/database/mod.rs"
VERSION="$(sed -n 's/.*SCHEMA_VERSION: i32 = \([0-9]*\);.*/\1/p' "$MOD_RS")"
if [ -z "$VERSION" ]; then
  echo "❌ SCHEMA_VERSION not found in $REF:src-tauri/src/database/mod.rs" >&2
  exit 1
fi
OUT="${2:-$ROOT/src-tauri/tests/fixtures/db/v$VERSION.sql}"

cat >>"$MOD_RS" <<'RUST'

#[cfg(test)]
mod ccs_fixture_gen {
    #[test]
    fn ccs_generate_fixture_db() {
        // HOME is redirected by gen-db-fixture.sh; init() creates ~/.cc-switch/cc-switch.db.
        let db = super::Database::init().expect("init fixture database");
        drop(db);
    }
}
RUST

if [ "${CCS_FIXTURE_LOCKFILE:-}" = "current" ]; then
  cp "$ROOT/src-tauri/Cargo.lock" "$WT/src-tauri/Cargo.lock"
fi

# tauri::generate_context! needs the frontend dist directory to exist.
mkdir -p "$WT/dist"
[ -f "$WT/dist/index.html" ] || echo '<!doctype html>' >"$WT/dist/index.html"

FAKE_HOME="$WORK/home"
mkdir -p "$FAKE_HOME"
echo "▶ building $REF ($SHA, schema v$VERSION)" >&2
HOME="$FAKE_HOME" CC_SWITCH_TEST_HOME="$FAKE_HOME" \
  cargo test --manifest-path "$WT/src-tauri/Cargo.toml" --lib \
  ccs_fixture_gen::ccs_generate_fixture_db -- --test-threads=1 >&2

DB="$FAKE_HOME/.cc-switch/cc-switch.db"
if [ ! -f "$DB" ]; then
  echo "❌ $REF did not create $DB" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUT")"
python3 -I - "$DB" "$OUT" "$VERSION" "$REF" "$SHA" <<'PY'
import sqlite3
import sys

db_path, out_path, version, ref, sha = sys.argv[1:6]
version = int(version)
conn = sqlite3.connect(db_path)

stored = conn.execute("PRAGMA user_version").fetchone()[0]
if stored != version:
    sys.exit(f"user_version {stored} != SCHEMA_VERSION {version} at {ref}")

# Sample user data. Each row is filtered to the columns that exist in this
# schema; a NOT NULL column without a default that the sample does not cover
# fails loudly so the sample gets extended instead of silently skipped.
TS = 1735689600  # 2025-01-01T00:00:00Z
SAMPLES = {
    "providers": [
        dict(id="claude-relay", app_type="claude", name="Claude Relay",
             settings_config='{"env":{"ANTHROPIC_BASE_URL":"https://relay.example.com","ANTHROPIC_AUTH_TOKEN":"sk-claude-fixture"}}',
             website_url="https://relay.example.com", category="third_party",
             created_at=TS, sort_index=0, notes="fixture current provider",
             meta='{"usage_script":null}', is_current=1, in_failover_queue=1),
        dict(id="claude-backup", app_type="claude", name="Claude Backup",
             settings_config='{"env":{"ANTHROPIC_BASE_URL":"https://backup.example.com"}}',
             category="custom", created_at=TS + 1, sort_index=1, meta="{}",
             is_current=0, in_failover_queue=0),
        dict(id="codex-main", app_type="codex", name="Codex Main",
             settings_config='{"auth":{"OPENAI_API_KEY":"sk-codex-fixture"},"config":"model = \\"gpt-5\\"\\n"}',
             category="third_party", created_at=TS + 2, sort_index=0, meta="{}",
             is_current=1),
        dict(id="gemini-main", app_type="gemini", name="Gemini Main",
             settings_config='{"env":{"GEMINI_API_KEY":"gm-fixture"}}',
             created_at=TS + 3, sort_index=0, meta="{}", is_current=1),
    ],
    "provider_endpoints": [
        dict(provider_id="claude-relay", app_type="claude",
             url="https://relay-2.example.com", added_at=TS),
    ],
    "mcp_servers": [
        dict(id="fetch", name="fetch",
             server_config='{"type":"stdio","command":"uvx","args":["mcp-server-fetch"]}',
             description="Fetch MCP", homepage="https://example.com/fetch",
             tags='["web"]', enabled_claude=1, enabled_codex=1, enabled_gemini=0),
    ],
    # v17+: per-app enablement lives in join tables (the enabled_* columns above
    # are still dual-written); rows match those flags
    "mcp_server_apps": [
        dict(server_id="fetch", app_type="claude"),
        dict(server_id="fetch", app_type="codex"),
    ],
    "skill_apps": [
        dict(skill_id="local:fixture-skill", app_type="claude"),
    ],
    "prompts": [
        dict(id="prompt-claude", app_type="claude", name="House rules",
             content="# Rules\nBe concise.", description="fixture prompt",
             enabled=1, created_at=TS, updated_at=TS),
    ],
    "skill_repos": [
        dict(owner="fixture-org", name="fixture-skills", branch="dev", enabled=0),
    ],
    "settings": [
        dict(key="common_config_claude", value='{"includeCoAuthoredBy":false}'),
        dict(key="fixture_marker", value=f"v{version}"),
    ],
    "proxy_request_logs": [
        dict(request_id="req-fixture-1", provider_id="claude-relay", app_type="claude",
             model="fixture-model", request_model="fixture-model-alias",
             input_tokens=1200, output_tokens=345, cache_read_tokens=100,
             cache_creation_tokens=50, input_cost_usd="0.0036",
             output_cost_usd="0.005175", cache_read_cost_usd="0.00003",
             cache_creation_cost_usd="0.0001875", total_cost_usd="0.0089925",
             latency_ms=850, first_token_ms=320, duration_ms=850, status_code=200,
             session_id="sess-fixture", provider_type="claude", is_streaming=1,
             cost_multiplier="1.0", created_at=TS),
    ],
    "usage_daily_rollups": [
        dict(date="2024-12-31", app_type="claude", provider_id="claude-relay",
             model="fixture-model", request_count=3, success_count=3,
             input_tokens=3000, output_tokens=900, cache_read_tokens=0,
             cache_creation_tokens=0, total_cost_usd="0.0225", total_cost_micros=22500,
             avg_latency_ms=700),
    ],
    "stream_check_logs": [
        dict(provider_id="claude-relay", provider_name="Claude Relay", app_type="claude",
             status="operational", success=1, message="ok", response_time_ms=420,
             http_status=200, model_used="fixture-model", retry_count=0, tested_at=TS),
    ],
    "proxy_live_backup": [
        dict(app_type="claude", original_config='{"env":{}}', backed_up_at="2025-01-01T00:00:00Z"),
    ],
    "session_log_sync": [
        dict(file_path="/fixture/session.jsonl", last_modified=TS,
             last_line_offset=42, last_synced_at=TS),
    ],
    "profiles": [
        dict(id="profile-fixture", name="Fixture project", payload='{"claude":{}}',
             sort_order=0, created_at=TS, updated_at=TS),
    ],
}
# Skills changed shape twice: v1 keys install state by "app:directory", v2 by
# (directory, app_type), v3+ keeps one row per skill with per-app flags.
LEGACY_SKILL = dict(key="claude:fixture-skill", installed=1, installed_at=TS)
PER_APP_SKILL = dict(directory="fixture-skill", app_type="claude", installed=1,
                     installed_at=TS)
UNIFIED_SKILL = dict(id="local:fixture-skill", name="Fixture Skill",
                     description="fixture", directory="fixture-skill",
                     enabled_claude=1, enabled_codex=0, enabled_gemini=0,
                     installed_at=TS, updated_at=TS)


def columns(table):
    return conn.execute(f'PRAGMA table_info("{table}")').fetchall()


def insert(table, row):
    info = columns(table)
    if not info:
        return
    names = {c[1] for c in info}
    data = {k: v for k, v in row.items() if k in names}
    missing = [c[1] for c in info
               if c[3] and c[4] is None and c[1] not in data
               and not (c[5] and c[2].upper() == "INTEGER")]
    if missing:
        sys.exit(f"sample row for {table} lacks NOT NULL columns {missing}; extend SAMPLES")
    cols = ", ".join(f'"{k}"' for k in data)
    marks = ", ".join("?" for _ in data)
    conn.execute(f'INSERT OR REPLACE INTO "{table}" ({cols}) VALUES ({marks})',
                 list(data.values()))


for table, rows in SAMPLES.items():
    for row in rows:
        insert(table, row)
skill_cols = {c[1] for c in columns("skills")}
if "key" in skill_cols:
    insert("skills", LEGACY_SKILL)
elif "id" not in skill_cols:
    insert("skills", PER_APP_SKILL)
else:
    insert("skills", UNIFIED_SKILL)
conn.commit()

with open(out_path, "w", encoding="utf-8", newline="\n") as f:
    f.write(f"-- cc-switch schema v{version} fixture (CCS-011)\n")
    f.write(f"-- source: {ref} ({sha})\n")
    f.write("-- generated by scripts/gen-db-fixture.sh; do not edit by hand\n")
    f.write(f"PRAGMA user_version = {version};\n")
    f.write("-- model_pricing seed rows are omitted: the app re-seeds them on start\n")
    for line in conn.iterdump():
        if line.startswith('INSERT INTO "model_pricing" '):
            continue
        f.write(line + "\n")
print(f"wrote {out_path} (schema v{version})")
PY
