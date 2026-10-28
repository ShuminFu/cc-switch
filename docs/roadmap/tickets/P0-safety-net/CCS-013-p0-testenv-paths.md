# CCS-013: [tests] Introduce a hermetic TestEnv with injected Paths and remove the global test_mutex

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P0`, `p1-high`, `area:tests`

> **Ticket key:** `p0-testenv-paths` · **Phase:** P0 · **Priority:** p1-high · **Effort:** L
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Integration tests mutate global HOME and serialize on test_mutex, so they cannot run in parallel and they leak state between tests.

## Evidence
- src-tauri/tests/support.rs:7-67 (HOME override + test_mutex)
- config.rs and settings.rs resolve paths via dirs::home_dir directly

## Proposed change
- Add a `Paths` struct injected into config.rs and settings.rs.
- Add `TestEnv { home: TempDir, db, settings }`.
- The production default `Paths` must resolve exactly as dirs::home_dir does today. Add a test that asserts the resolved paths for every app match.

## Acceptance criteria / verification
- [x] test_mutex is deleted; `cargo test --manifest-path src-tauri/Cargo.toml` passes with default parallelism, run 5 times in a row
- [x] A path-equivalence test covers all 8 apps
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

## Dependencies
None.

## Implementation notes

**Summary**
- New `src-tauri/src/paths.rs`: `Paths` (home + derived `app_config_dir`, `settings_path`, `claude_mcp_path`, `default_app_dir(&AppType)` for all 8 apps). `Paths::from_env()` is the production default and resolves exactly as before (`CC_SWITCH_TEST_HOME` if set, else `dirs::home_dir()`, fallback `.`). `paths::current()` returns a thread-scoped injected `Paths` if present, else `from_env()`; `paths::scoped()` returns a guard that restores the previous value on drop.
- `config.rs` (`get_home_dir`, `get_claude_config_dir`, `get_default_claude_mcp_path`, `get_app_config_dir`) and `settings.rs` (`settings_path`, `~` expansion in `resolve_override_path`, which previously called `dirs::home_dir()` directly) resolve through `paths::current()`. The codex/gemini/grok/opencode/openclaw/hermes default-dir getters use `Paths::default_app_dir`.
- `settings.rs`: the settings cache is now `Arc<RwLock<_>>` and can be replaced per thread via `scoped_settings_store()` (guard `ScopedSettingsStore`).
- `tests/support.rs`: `test_mutex`, `ensure_test_home` and `reset_test_fs` are deleted. `TestEnv::new()` creates its own `TempDir` HOME and injects both `Paths` and a fresh settings store for the current thread. The DB comes from `create_test_state*()`, which now resolves into the TestEnv HOME. As a safety net, the process-level `CC_SWITCH_TEST_HOME`/`HOME` point once at a per-process temp dir, so code on a thread without the injection never touches the real home. After a full run this dir was never created, so no test reached it.
- All 11 integration test files moved to `TestEnv`. `export_sql_returns_error_for_invalid_path` now uses a path whose parent is a regular file inside the TestEnv HOME, so it no longer fails when `/nonexistent` exists (running as root).
- New tests: `paths::tests::production_default_paths_match_dirs_home_dir_for_every_app` (path equivalence for all 8 apps plus app dir/settings/MCP paths against `dirs::home_dir()`), `test_home_env_override_is_honored`, `scoped_paths_are_thread_local_and_restored_on_drop`, `every_app_dir_is_rooted_in_injected_home`. Also `tests/test_env_isolation.rs`: two TestEnvs on parallel threads, each with its own settings override and DB row, check after a barrier that neither sees the other's data, and that each HOME is removed on drop. A second test checks that nested TestEnvs restore the previous paths.
- Limitation: the injection is thread-local. Tests must keep the default current_thread tokio runtime, or create the TestEnv on the thread that does the work. Lib unit tests that use `CC_SWITCH_TEST_HOME` + `#[serial]` are unchanged (they still work; migrating them is out of scope).

**Files**
`src-tauri/src/paths.rs` (new), `src-tauri/src/{config,settings,lib,codex_config,gemini_config,grok_config,opencode_config,openclaw_config,hermes_config}.rs`, `src-tauri/tests/support.rs`, `src-tauri/tests/test_env_isolation.rs` (new), `src-tauri/tests/{app_config_load,deeplink_import,hermes_roundtrip,import_export_sync,mcp_commands,profile_roundtrip,provider_commands,provider_service,proxy_commands,skill_sync}.rs`.

**Commits**
- `999eb32` test(rust): hermetic TestEnv with injected Paths, drop global test_mutex (CCS-013)

**Verification (Linux)**
- `cargo test --manifest-path src-tauri/Cargo.toml` (default parallelism), 5 runs in a row: each run 2141 passed, 0 failed across 15 test binaries. This includes `import_export_sync`, which failed before this change in this environment.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean. `cargo clippy ... -- -D warnings`: clean.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.
- Not run here: the Windows and macOS `cfg` branches in `paths.rs` (Claude Desktop / Hermes defaults, the matching legacy expressions in the equivalence test). Only the Linux target is installed. These branches mirror the existing per-platform code and need a CI run on the Windows/macOS runners.

**Review**
- Checked each acceptance criterion against `999eb32`. `test_mutex`, `ensure_test_home` and `reset_test_fs` are gone. `paths::tests::production_default_paths_match_dirs_home_dir_for_every_app` covers all 8 apps. Production resolution is unchanged: `CC_SWITCH_TEST_HOME` if set, else `dirs::home_dir()`, else `.`, and the v3.10.3 Windows `HOME` fallback still applies. The only behaviour difference is that `~` expansion in settings overrides now also honors `CC_SWITCH_TEST_HOME`. That variable is test-only.
- Follow-up: removed the stale `#[allow(clippy::await_holding_lock)]` attributes and their "serialize with Mutex" comments in `tests/proxy_commands.rs` and `tests/provider_service.rs`. No lock is held across `.await` any more.
- Re-verified on Linux: full `cargo test` 4 runs in a row with default parallelism, each 2141 passed / 0 failed. The fallback HOME was never created. `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean. Known gaps, not fixed here: the Windows/macOS cfg branches still need CI. Some code outside config/settings still calls `dirs::home_dir()` directly (`prompt_files`, `claude_plugin`, `app_store`, `env_manager`, `panic_hook`, `commands/misc`, `session_manager/providers/opencode`), and Windows Claude Desktop/Hermes read `LOCALAPPDATA` directly. None of this is in the ticket's stated scope, and it behaves as it did before.
