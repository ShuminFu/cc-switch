# CCS-012: [apps] Add round-trip snapshots for all live-config writers, OAuth token stores and the Codex state DB

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:apps-tests`

> **Ticket key:** `p0-config-writer-snapshots` · **Phase:** P0 · **Priority:** p1-high · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Takeover and adapter refactors will rewrite users' CLI configs, and nothing pins byte-stability today. The critique adds two gaps: the OAuth token stores (own atomic writers) and the Codex state DB written by codex_history_migration.rs.

## Evidence
- Writers: claude settings.json, ~/.claude.json MCP (claude_mcp.rs:345), codex config.toml/auth.json (codex_config.rs:244), gemini .env/settings.json, grok, opencode, openclaw, hermes, claude_desktop, claude_plugin.rs:83/117
- proxy/providers/copilot_auth.rs:1247-1294 and codex_oauth_auth.rs (OpenOptions writers)
- codex_history_migration.rs:644,1157 updates threads.model_provider using CC_SWITCH_CODEX_MODEL_PROVIDER_ID
- Existing pattern: tests/hermes_roundtrip.rs

## Proposed change
- Add tests/config_writers.rs with tests/fixtures/live/** inputs that include comments, unknown keys, CRLF and a BOM.
- Snapshot the output, and assert that untouched sections are byte-identical.
- Snapshot the token-store JSON shape and file mode.
- Snapshot the model_provider id and the settings.json migration markers.

## Acceptance criteria / verification
- [x] 11 writers + 2 token stores + the Codex state DB covered
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test config_writers`

## Dependencies
None (p0-testenv-paths makes it hermetic later).

## Implementation notes

**Summary.** Added `src-tauri/tests/config_writers.rs`, which drives every
live-config writer, both OAuth token stores and the Codex state DB migration
through their production code paths inside an isolated `TestEnv` HOME (every
app config dir pinned below it, so the layout matches on all platforms) and
snapshots each touched file with insta. Every case runs three times: on the
checked-in LF fixture, on a CRLF conversion and on a BOM-prefixed copy. The
variants are derived at runtime, so `.gitattributes` normalisation can never
change the inputs. In snapshots `<BOM>`, `<CR>` and `<HOME>` make bytes
visible. Sections a writer must not touch are also asserted to be
byte-identical (`~/.claude.json` non-MCP keys, Hermes and OpenClaw comments
and other sections, the Codex `auth.json` when official auth is preserved).
A Unix-only `file_modes.snap` pins modes: 0600 for the token stores and
Gemini `.env`, 0700 for `~/.gemini`, and the seeded mode kept by every
`atomic_write` writer. A guard test fails if a writer loses its
fixture directory or a variant snapshot.

Coverage (fixture dir → entry point):
- Claude `settings.json` (`claude_settings`), `~/.claude.json` MCP
  (`claude_mcp`, `set_mcp_servers_map`), Codex `config.toml` + `auth.json`
  (`codex`, plus `codex_preserve_auth` with
  `preserve_codex_official_auth_on_switch`), Gemini `.env` + `settings.json`
  (`gemini`), Grok (`grok`), OpenCode (`opencode`), OpenClaw (`openclaw`),
  Hermes (`hermes`). These all go through `write_live_with_common_config`, the
  same path a provider switch uses.
- Claude Desktop (`claude_desktop`) through a new testkit entry
  `apply_provider_at_dirs` that applies a provider to explicit directories,
  so the writer also runs on Linux. Claude plugin `config.json`
  (`claude_plugin`, write then clear).
- Token stores: `CopilotAuthManager` (`copilot_auth`) and `CodexOAuthManager`
  (`codex_oauth`): JSON shape after `set_default_account` and mode 0600.
- Codex state DB (`codex_state_db`):
  `maybe_migrate_codex_third_party_history_provider_bucket` over a seeded
  `state_5.sqlite` and a session JSONL. The snapshot shows `threads.model_provider`
  rewritten to `CC_SWITCH_CODEX_MODEL_PROVIDER_ID` (`custom`), the JSONL
  rewrite and the `localMigrations` marker in `settings.json` (timestamp
  redacted). The test also asserts that a state-DB backup is written.

The lib exposes a `#[doc(hidden)]` module `live_config_testkit`, compiled only
under `cfg(any(test, feature = "testkit"))`. It re-exports these entry points
and adds no behaviour. The Claude Desktop path helpers' cfg gates were widened
to include `feature = "testkit"`.

**Current behaviour these snapshots pin (follow-up candidates, not fixed here):**
- **OpenClaw corrupts block comments (data-loss bug).** The json-five 0.3.1
  round-trip serializer drops the `/` that closes a `/* ... */` comment. The
  rewritten `openclaw.json` no longer parses, and the write returns an error
  *after* the file has already been replaced (the previous file survives
  only in `~/.cc-switch/backups/openclaw`). This is pinned with an inverted
  assertion in `openclaw_json5`, which will fail once the bug is fixed.
- BOM-prefixed JSON: `claude_plugin` and Gemini `settings.json` silently treat
  the file as `{}` and drop user keys (`mcpServers`, `theme`, ...). Both
  token stores fail to load, so the account looks logged out
  (`AccountNotFound`). `~/.claude.json` and Claude Desktop refuse the write
  with a parse error. In the Codex state DB the BOM'd JSONL line is skipped.
- CRLF: the Hermes section replacement leaves mixed line endings. JSON
  writers, Codex, Gemini and Grok normalise to LF.
- Writers that reserialize (Claude `settings.json`, Gemini `settings.json`,
  OpenCode and Claude Desktop) sort keys. OpenCode drops JSONC comments,
  Gemini `.env` drops comments and the trailing newline, and Codex
  `auth.json` is rewritten whole. Token stores drop unknown fields. Codex
  `auth.json` keeps the existing mode (0644 here) even though it holds API
  keys.
- `set_mcp_servers_map`, the token stores and OpenClaw typed providers
  serialize `HashMap`s, so their byte order is only stable with a single
  entry. The fixtures therefore use one server and one account.

**Files.** `src-tauri/tests/config_writers.rs`,
`src-tauri/tests/fixtures/live/**` (inputs + `lf|crlf|bom.snap` per writer,
`file_modes.snap`), `src-tauri/src/live_config_testkit.rs`,
`src-tauri/src/lib.rs`, `src-tauri/src/claude_desktop_config.rs`.

**Commits.** 994f30d (tests, fixtures, testkit), plus the commit that records
these notes.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --test config_writers`:
  16 passed. Run 3× with `CI=true`: stable. No `.snap.new` files left.
- Mutation check: making `serialize_env_file` append a trailing newline made
  `gemini_env_and_settings_json` fail. The mutation was reverted.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2078 passed, 3 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml --tests --no-fail-fast`:
  all green except one intermittent failure of
  `migrations::every_released_schema_migrates_to_current` (CCS-011's test).
  It passed 4/4 when run alone and is unrelated to this change.
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and
  `--all-targets -- -D warnings`: clean.
- Not run locally: the Windows and macOS CI legs. The snapshots avoid
  platform-specific content (paths are redacted with backslashes normalised,
  modes appear only in the Unix-only snapshot, MCP fixtures avoid the
  commands that get the Windows `cmd /c` wrapper). Still, the first Windows
  and macOS CI run should be watched.

**Review.** All acceptance criteria were checked against the code. Every
writer goes through its production entry point, the token stores go through
their 0600 `OpenOptions` writers, and the testkit adds no behaviour. One gap
was fixed: the Codex state DB fixture's legacy provider had no
`[model_providers.ccswitch]` table, so the migration never trusted the legacy
`ccswitch` id. The `t2-ccswitch` row, titled "migrated (legacy id)", stayed
on `ccswitch`, and the legacy-bucket path was not covered. The fixture
provider now defines that table, and the snapshots pin both `aihubmix` and
`ccswitch` being rewritten to `custom` (`migratedStateRows: 2`). The
re-verification passed: `--test config_writers` 3× with `CI=true` (16 passed,
no `.snap.new`), `--lib` (2078 passed), and `cargo fmt` and `cargo clippy`
(plain and `--all-targets`, `-D warnings`) were clean.
