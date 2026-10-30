# CCS-167: [usage] Add budget alerts on usage-log-recorded

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:usage`

> **Ticket key:** `p4-budget-alerts` · **Phase:** P4 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p4-usage-repo`, `p1-spend-limits-correct`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Limits are pull-only today.

## Proposed change
On each usage-log-recorded event, evaluate the provider's daily and monthly limits (from p1-spend-limits-correct) and send a system notification at 80% and 100%, deduped per period.

## Acceptance criteria / verification
- [x] Test: crossing 80% emits one alert per period
- [x] `cargo test --manifest-path src-tauri/Cargo.toml budget` and `pnpm test:unit`

## Dependencies
p4-usage-repo, p1-spend-limits-correct

## Implementation notes

**Summary.** Added `services::budget_alerts`. The debounced `usage-log-recorded` emitter
(`usage_events::notify_log_recorded`) now also calls `budget_alerts::on_usage_recorded`
after the emit, in the same background thread. The handler lists every provider whose meta
has `limitDailyUsd` / `limitMonthlyUsd`, gets usage for the local day and local month from
`check_provider_limits_at` (the windows fixed in p1-spend-limits-correct), and sends one
system notification (via `tauri-plugin-notification`, called from Rust only, so no capability
or JS package is needed) when a period reaches 80% (`Warning`) or 100% (`Exceeded`).
- Dedup happens per `(app_type, provider_id, daily|monthly)` and period key (`YYYY-MM-DD` /
  `YYYY-MM`). Each level fires at most once per period, and a jump from under 80% straight
  past 100% sends only the 100% alert. A new period resets the level.
- The dedup state is a JSON row in `settings` (`usage_budget_alert_state`). It is added to
  `LOCAL_ONLY_SETTING_KEYS`, so it is never exported or synced, and an import keeps the local
  value. Entries for providers that no longer have limits are pruned.
- Evaluations are serialised with a mutex. Any failure is logged at warn level and never
  affects log writing. Notification text follows `settings.language` (zh / zh-TW / en / ja),
  falling back to the system locale the same way the tray does.

**Files.** `src-tauri/src/services/budget_alerts.rs` (new), `src-tauri/src/services/mod.rs`,
`src-tauri/src/usage_events.rs`, `src-tauri/src/database/repo/usage.rs`
(`list_providers_with_spend_limits`), `src-tauri/src/database/backup.rs`,
`src-tauri/src/tray.rs` (`detect_system_tray_language` made `pub(crate)`),
`src-tauri/src/lib.rs` (plugin registration), `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`.
No files under `src-tauri/src/proxy/**` were touched.

**Commits.** 90a7891 (feature + tests); this notes edit is in a follow-up commit.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib budget`: 33 passed, 0 failed. This
  includes 6 new `services::budget_alerts::tests::*`. The main one is
  `budget_crossing_80_percent_alerts_once_per_period`, which checks that one alert fires at
  80%, there is no repeat in the same period (including after the persisted state is reloaded),
  the alert escalates once at 100%, and the daily period resets the next day. Other tests
  cover monthly dedup across days, a direct jump past 100%, providers without limits, state
  pruning and localized text.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2650 passed, 0 failed
  (lib suite 2449 passed / 3 ignored).
- `cargo fmt` clean; `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` clean.
- `pnpm test:unit`: 159 files / 1542 tests passed (frontend unchanged).
- Not verified here: whether the OS actually displays the notification on Windows and macOS
  (macOS needs the bundled app identity). This needs a manual check on those platforms.
