# CCS-147: [apps] Migrate Claude and Claude Desktop to AppAdapter

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:apps`

> **Ticket key:** `p4-adapter-claude` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-adapter-trait-gemini-grok`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Add apps/claude and apps/claude_desktop.
- Move settings.json, ~/.claude.json and the plugin writer.
- Remove their arms from live.rs, prompt_files.rs, skill.rs:500-550, commands/config.rs:90-180, commands/provider.rs:147-200 and services/provider/mod.rs.

## Acceptance criteria / verification
- [x] Writer snapshots are identical
- [x] The match-app count drops (track the number in the PR)
- [x] `cargo test --manifest-path src-tauri/Cargo.toml`

## Dependencies
p4-adapter-trait-gemini-grok

## Implementation notes

**Summary.** Added `apps/claude` (`ClaudeAdapter`) and `apps/claude_desktop` (`ClaudeDesktopAdapter`). `apps::adapter()` now returns adapters for Claude, Claude Desktop, Gemini and Grok Build.

- Moved files: `claude_mcp.rs` (the `~/.claude.json` writer) became `apps/claude/claude_json.rs`, `claude_plugin.rs` (the plugin writer) became `apps/claude/plugin.rs`, and `claude_desktop_config.rs` became `apps/claude_desktop/config.rs`. `lib.rs` keeps `crate::claude_mcp`, `crate::claude_plugin` and `crate::claude_desktop_config` as `use` aliases, so other callers (including `src/proxy/**`) did not change.
- The settings.json code moved into `ClaudeAdapter`: read/import (including legacy model-key normalization), the sanitized write, validation, credentials, the common-config snippet (extract/contains/apply/remove, plus `is_sensitive_config_key`, now in `apps/claude/common_config.rs`), MCP projection, the takeover placeholder check and session roots (`projects/`). The Codex OAuth / Kimi For Coding context-window live defaults and their symmetric backfill strip moved to `apps/claude/live_defaults.rs`. `services::provider::sanitize_claude_settings_for_live` stays as a re-export.
- `ClaudeDesktopAdapter` handles validation and direct-gateway credentials. It has no prompt file, skills dir, MCP, takeover or sessions. Its generic read/import/`write_provider` return the same errors as before. The real write goes through `write_effective_provider(db, …)`, which calls `apply_provider`.
- Trait changes (`apps/adapter.rs`):
  - `config_dir()` now returns `Result<PathBuf>`, because the Claude Desktop config library is platform-gated.
  - `prompt_file()` and `skills_dir()` now return `Option<PathBuf>`.
  - New methods: `write_effective_provider`, `apply_live_defaults`, `normalize_settings`, `takeover_uses_live_backup` (false for Claude Desktop) and `CommonSnippet::sync_from_live` (true for Claude, false for Gemini).
- Removed arms:
  - `live.rs`: common-config contains/apply/remove, effective-settings defaults, backfill restore, `write_live_with_common_config`, `write_live_snapshot`, the takeover-aware sync, `read_live_settings` and `import_default_config`. The never-constructed `LiveSnapshot::Claude` variant was deleted.
  - `services/provider/mod.rs`: normalize, validate, credentials, common-config extract, sync-from-live scope, and the three Claude Desktop takeover bypasses (these now use `takeover_uses_live_backup`).
  - Other callers: `prompt_files.rs`, `services/skill.rs::get_app_skills_dir` and `commands/config.rs`. The two copies of the per-app config-dir match in `commands/config.rs` were folded into `app_config_dir()`, which also drops the Gemini/Grok arms. Also `services/mcp.rs` (single-server upsert/remove) and `session_manager::provider_roots`.
  - The proxy hot refresh after a provider save (Claude always, Codex only when live is taken over) moved from `services/provider/mod.rs` into `ProxyService::refresh_live_after_provider_save` in `services/proxy.rs`. `is_claude_live_taken_over` delegates to the adapter. `src/proxy/**` is untouched.
- `settings::get_claude_override_dir` was removed. Its callers (`config.rs`, `apps/claude/plugin.rs`, the Windows-only `commands/misc.rs::wsl_distro_for_tool`) now use `settings::override_dir(&AppType::Claude)`.

**Match-app count** is the number of `AppType::Claude` / `AppType::ClaudeDesktop` references in production code, with test modules and comments excluded:

| Scope | Before | After |
|---|---|---|
| Whole crate | 154 | 123 |
| `services/provider/live.rs` | 13 | 0 |
| `services/provider/mod.rs` | 12 | 0 |
| `commands/config.rs` | 4 | 0 |
| `prompt_files.rs` | 1 | 0 |
| `services/mcp.rs` | 6 | 2 |
| `services/skill.rs` | 7 | 3 |

`services/proxy.rs` went from 21 to 23, because it now holds the moved hot-refresh dispatch and the adapter delegation.

**Intentionally left:**
- `provider.rs::resolve_usage_credentials` (the plan's "provider.rs:147-200"). It is usage-script credential resolution, which is not part of the adapter trait, and CCS-146 left its Gemini/Grok arms in place too.
- `commands/provider.rs` Claude Desktop import. It uses DB slugs, not per-app dispatch.
- The `ClaudeDesktop` skill-sync guards in `skill.rs` (capability checks).
- `.github/labeler.yml` still lists `src-tauri/src/claude_mcp.rs`. That file belongs to the CI lane, so I did not edit it. Point it at `src-tauri/src/apps/claude/claude_json.rs`.

**Commits.** `7a224db` refactor(apps): migrate Claude and Claude Desktop to AppAdapter (CCS-147), plus this ticket update.

**Tests** (`apps::tests`):
- `adapter_backed_apps_have_no_arms_in_provider_service` now also scans `prompt_files.rs` and `commands/config.rs`, and covers Claude and Claude Desktop. It fails on the pre-change sources: 13 hits in live.rs, 12 in mod.rs, 1 in prompt_files.rs and 4 in commands/config.rs. It passes now.
- `migrated_apps_have_adapters` (renamed from `only_gemini_and_grok_have_adapters`).
- `adapter_paths_match_legacy_layout` was extended to Claude and Claude Desktop. It checks that the Claude prompt path equals the old `settings path parent + CLAUDE.md`.
- `takeover_placeholder_detection` was extended to the four Claude takeover keys.
- New: `claude_credentials_prefer_auth_token`, `claude_common_snippet_round_trips_and_syncs_from_live`, `claude_normalizes_legacy_small_fast_model`, `claude_desktop_live_io_requires_switch_flow`.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: all green.
  - lib: 2334 passed, 0 failed, 2 ignored.
  - config_writers: 17 passed. The writer snapshots are unchanged, so the output is identical.
  - provider_service 33, provider_commands 10, mcp_commands 20, import_export_sync 26, skill_sync 8, proxy_e2e 19, proxy_golden 6 and write_lint 3: all passed. Every other integration binary also passed.
- The Windows-only `wsl_distro_for_tool` edit and the macOS/Windows-only Claude Desktop path code were not compiled locally (Linux runner). Both are mechanical changes.

