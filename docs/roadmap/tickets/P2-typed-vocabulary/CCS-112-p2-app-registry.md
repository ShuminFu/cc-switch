# CCS-112: [apps] Add the AppDescriptor registry and replace hard-coded app lists

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p1-high`, `area:apps`

> **Ticket key:** `p2-app-registry` · **Phase:** P2 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p0-config-writer-snapshots`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
About 15 hand-kept app lists drift apart. Adding Grok Build touched 112 files.

## Evidence
- lib.rs:1754, lib.rs:788-819, services/proxy.rs:1308, tray.rs:157; app_config.rs:28-35

## Proposed change
- Add apps/registry.rs with `static APPS: &[AppDescriptor]` (slug, aliases, label key, SyncMode, AppCaps bitflags, proxy_route, input_token_semantics, default_visible).
- AppType::as_str, FromStr, is_additive_mode and all() delegate to it.
- Replace the listed sites with APPS.filter(caps).

## Acceptance criteria / verification
- [ ] No hard-coded app list at the 4 sites (grep check in CI)
- [ ] Config-writer snapshots, proxy_e2e and migrations are unchanged
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`

## Dependencies
p0-config-writer-snapshots

## Implementation notes

**Summary.** Added `src-tauri/src/apps/registry.rs` with `pub static APPS: &[AppDescriptor]`
(fields: `app`, `slug`, `aliases`, `label_key`, `display_name`, `sync_mode: SyncMode`,
`caps: AppCaps`, `proxy_route`, `input_token_semantics: InputTokenSemantics`,
`default_visible`) plus helpers `descriptor()`, `find_by_id()` and `with_caps()`.
`AppCaps` is a small hand-rolled bitflag newtype (`MCP`, `SKILLS`, `PROMPTS`,
`PROXY_TAKEOVER`, `TRAY`) so no new crate dependency was needed.

- `AppType::as_str`, `FromStr` (slug + aliases, error message's allowed list), `is_additive_mode`
  and `all()` delegate to the registry; new `AppType::descriptor()`.
- Former hard-coded sites now filter `APPS` by caps:
  - `lib.rs` first-launch prompt import -> `with_caps(PROMPTS)`;
  - `lib.rs` `PROXY_STARTUP_APP_TYPES` removed -> `with_caps(PROXY_TAKEOVER)`;
  - `services/proxy.rs` `sync_live_to_providers` loops over `with_caps(PROXY_TAKEOVER)` using a
    new `read_live_for_takeover()` helper (shared with `sync_live_to_provider`);
  - `tray.rs` `TRAY_SECTIONS` is now a `Lazy<Vec<TrayAppSection>>` built from `with_caps(TRAY)`
    (prefix/empty id derived from the slug, labels from `display_name`);
  - `app_config.rs` `McpApps::enabled_apps` / `SkillApps::enabled_apps` derive from `with_caps(MCP|SKILLS)`;
  - `services/sql_helpers.rs` cache-inclusive app list derives from `input_token_semantics`.
- The legacy common-config migration list in `lib.rs` (claude/codex/gemini) was intentionally left
  as-is (migration behaviour must not change).
- "Grep check in CI": implemented as the unit test
  `apps::registry::tests::former_hard_coded_app_lists_stay_removed`, which `include_str!`s the four
  sites (+ `app_config.rs`) and fails if the old literal lists reappear; it runs in the existing
  CI `cargo test` job, so no workflow change was needed. Verified the patterns match the
  pre-change sources (HEAD~) and not the new ones.
- Further registry tests: every variant registered once, slug/alias uniqueness, as_str/FromStr
  round trip, slug == serde form, additive mode, capability sets equal the historical lists,
  MCP/Skill `is_enabled_for` respects caps, `default_visible` == `VisibleApps::default()`,
  `proxy_route`s are mounted in `proxy/server.rs`, `label_key`s exist in `en.json`.
- Observation (not changed, out of scope): `services/usage_stats.rs` treats only `codex|gemini`
  as cache-inclusive in its in-memory path while the SQL helper also includes `grokbuild`.

**Files.** `src-tauri/src/apps/{mod.rs,registry.rs}` (new), `src-tauri/src/app_config.rs`,
`src-tauri/src/lib.rs`, `src-tauri/src/services/proxy.rs`, `src-tauri/src/services/sql_helpers.rs`,
`src-tauri/src/tray.rs`.

**Commits.** d22eca6 (code), plus this ticket-notes commit.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` -> clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` (and `--all-targets`) -> clean
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` -> 2209 passed, 0 failed, 3 ignored
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` -> 2367 passed, 0 failed
  (config-writer snapshots, proxy_e2e and migration tests unchanged; no snapshot files modified)
