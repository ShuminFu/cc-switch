# CCS-236: [apps] Move the remaining stream-check, terminal and proxy-provider dispatch into adapters and set the match-app budget to 15 or fewer

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p3-low`, `area:apps`

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
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib apps::tests::match_app_sites_outside_apps_stay_within_budget` passes with budget ≤ 15.
- [ ] `cargo test --test proxy_golden`, `--test proxy_e2e`, `--lib services::stream_check`, `--lib commands::terminal` pass.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` pass.

## Dependencies
CCS-233 (`p4-apps-ninth-app-six-files`), CCS-234 (`p4-takeover-dispatch-via-adapter`), CCS-235 (`p4-apps-config-profile-keyed-access`).
