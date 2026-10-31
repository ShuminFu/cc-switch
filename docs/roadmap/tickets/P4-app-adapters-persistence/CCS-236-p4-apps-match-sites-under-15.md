# CCS-236: [apps] Move the remaining stream-check, terminal and proxy-provider dispatch into adapters and set the match-app budget to 15 or fewer

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p3-low`, `area:apps`

> **Ticket key:** `p4-apps-match-sites-under-15` · **Phase:** P4 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p4-apps-ninth-app-six-files`, `p4-takeover-dispatch-via-adapter`, `p4-apps-config-profile-keyed-access`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-149, CCS-150, CCS-005). Source tickets: [CCS-149](../P4-app-adapters-persistence/CCS-149-p4-adapter-additive.md), [CCS-150](../P4-app-adapters-persistence/CCS-150-p4-adapter-conformance.md), [CCS-005](../P4-app-adapters-persistence/CCS-005-epic-p4.md).

## Problem
After CCS-233/234/235 the remaining single sites are `services/stream_check.rs:210` (`resolve_base_url`), `commands/terminal.rs:64` (`base_url_key`), `services/proxy/hot_switch.rs`/`backup.rs` leftovers if any, and `proxy/providers/mod.rs` (:95 `ProviderType::from_app_type_and_config`, :216 `get_adapter`). The CCS-149 criterion ("≤ 15 match-app sites outside apps/**") and the P4 epic criterion need the budget actually lowered, and `proxy/providers/mod.rs` is also one of the 9th-app files.

## Proposed change
- `stream_check.rs:210`: `AppAdapter::probe_base_url(provider)`.
- `commands/terminal.rs:64`: an adapter method returning the env base-url key (coordinate with CCS-179's terminal split).
- `proxy/providers/mod.rs`: make `get_adapter` a table or `AppAdapter::proxy_adapter()`, and give `from_app_type_and_config` a `_ => AdditiveAppAdapter` default, if still needed to reach the targets. Add a fixture change or `no-fixture-needed` label (CCS-034).
- Set `MATCH_APP_SITE_BUDGET` to the measured count (≤ 15 while legacy.rs's 2 sites remain) and `NEW_APP_BACKEND_FILE_BUDGET` to the measured count.
- Tick CCS-149's criterion with the new count; update CCS-005's epic table.

## Acceptance criteria / verification
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib apps::tests::match_app_sites_outside_apps_stay_within_budget` passes with budget ≤ 15.
- [x] `cargo test --test proxy_golden`, `--test proxy_e2e`, `--lib services::stream_check`, `--lib commands::terminal` pass.
- [x] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` pass.

## Dependencies
CCS-233 (`p4-apps-ninth-app-six-files`), CCS-234 (`p4-takeover-dispatch-via-adapter`), CCS-235 (`p4-apps-config-profile-keyed-access`).

## Implementation notes
**Summary.** Three new `AppAdapter` hooks replace the remaining `AppType` matches outside `apps/**` named by this ticket:
- `upstream_protocol() -> UpstreamProtocol` (`Anthropic` / `OpenAiResponses` / `Gemini` / `AppEntries`, default `AppEntries`). `proxy::providers::get_adapter` and `ProviderType::from_app_type_and_config` now match on it instead of `AppType`. Claude and Claude Desktop declare `Anthropic`, Codex and Grok Build `OpenAiResponses`, Gemini `Gemini`. OpenCode, OpenClaw and Hermes use the default, which maps to `AdditiveAppAdapter` and `ProviderType::Codex` as before.
- `probe_base_url(provider) -> Option<Result<String, AppError>>`. `StreamCheckService::resolve_base_url` uses it and falls back to the upstream adapter's `extract_base_url` when it returns `None`. The OpenCode (explicit `options.baseURL`, else the npm default endpoint), OpenClaw (`baseUrl`) and Hermes (`base_url`) extractors moved unchanged from `stream_check.rs` into their adapters. The separate Claude Desktop arm is gone because `get_adapter` already maps it to `ClaudeAdapter`.
- `env_base_url_key() -> Option<&'static str>`. Claude and Claude Desktop return `ANTHROPIC_BASE_URL`, Gemini returns `GOOGLE_GEMINI_BASE_URL`. `commands/terminal.rs` uses it. The terminal module is unchanged otherwise, so there is no conflict with the CCS-179 split.

**Budgets.** The counter now reports **4** sites, against a target of 15 or fewer: `deeplink/provider.rs`, `services/config.rs` and 2 in time-gated `services/proxy/takeover/legacy.rs`. `MATCH_APP_SITE_BUDGET` went from 8 to 4. `services/stream_check.rs`, `commands/terminal.rs` and `proxy/providers/mod.rs` were added to the never-match file list. `proxy/providers/mod.rs` has no exhaustive `AppType` match any more, so a 9th app touches **4** backend files: `app_config.rs`, `apps/registry.rs`, `apps/mod.rs` and the new adapter. `NEW_APP_BACKEND_FILE_BUDGET` went from 5 to 4. CCS-149's criterion is ticked and CCS-005's epic table and exit criteria are updated.

**Fixture (CCS-034).** `no-fixture-needed`. The proxy wire behavior is unchanged: the same `ProviderAdapter` and `ProviderType` are chosen for every app. `proxy_golden` and `proxy_e2e` pass unchanged.

**Tests.**
- The match-app budget test fails before this change (8 sites > 4) and passes after it.
- New `proxy::providers::tests::get_adapter_follows_app_upstream_protocol` and `test_from_app_type_openai_compatible_apps_are_codex` pin the previous per-app arms.
- New `services::stream_check::tests::test_resolve_base_url_dispatches_through_app_adapters` covers Hermes, Claude Desktop and Gemini. The existing OpenCode and OpenClaw tests now go through `resolve_base_url`.
- New `commands::terminal::tests` (2 tests) cover the base_url key per app and the Codex and Gemini API-key env vars.

**Files.** `src-tauri/src/apps/{adapter.rs,mod.rs,claude,claude_desktop,codex,gemini,grok,hermes,openclaw,opencode}/…`, `src-tauri/src/commands/terminal.rs`, `src-tauri/src/proxy/providers/mod.rs`, `src-tauri/src/services/stream_check.rs`, `src-tauri/tests/app_adapter_conformance.rs`, plus the docs for CCS-149, CCS-005 and this ticket.

**Commits.** d2d0685 (code), plus the docs commit that records these notes.

**Verification.**
- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings` and `cargo clippy -- -D warnings`: clean.
- `cargo test --lib -- apps:: services::stream_check commands::terminal proxy::providers`: 919 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 3114 passed, 0 failed across all targets. That includes `proxy_golden` (18 passed), `proxy_e2e` (37 passed) and `app_adapter_conformance` (13 passed).
