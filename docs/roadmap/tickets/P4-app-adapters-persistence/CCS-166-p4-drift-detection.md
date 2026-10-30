# CCS-166: [apps] Detect live-config drift and external edits, and offer reconcile

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:apps`

> **Ticket key:** `p4-drift-detection` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-takeover-engine`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Merges the plan backlog item with backend idea 4.
- Leftover takeover placeholders after a crash, and external edits to hooks or permissions, are lost on the next switch.

## Proposed change
- Watch the live files (notify crate) and compare them with the expected adapter output.
- On drift, offer: reconcile (repair placeholders), fold external fields into the common config, or ignore.

## Acceptance criteria / verification
- [x] Tests: a leftover PROXY placeholder after a simulated crash is detected and repaired; an externally added `hooks` key is offered for folding
- [x] `cargo test --manifest-path src-tauri/Cargo.toml drift` and `pnpm test:unit`

## Dependencies
p4-takeover-engine

## Implementation notes

**Summary.** New `services::drift` module. For each switch-mode app with proxy takeover (Claude, Codex, Gemini, Grok Build), `DriftService::detect` captures what re-writing live for the current provider would produce (`ProviderService::capture_live_write`, the same capture path as the CCS-205 preview, including the MCP re-projection), then compares it key by key with the files on disk. JSON, TOML and `.env` files are compared to a depth of two (`hooks`, `env.FOO`).
- A **leftover placeholder** is a live config that still has the adapter's takeover placeholder (`ProxyTakeover::has_placeholder`) while the proxy is not taking that app over (proxy not running, or `enabled` is off).
- Each **field drift** (`added` / `modified` / `removed`) gets a `foldable` flag. A field is foldable only when it survives the app's `CommonSnippet::extract` of the live config, so credentials and provider-specific fields never qualify.
- While the takeover is active, only new top-level keys are reported, because the takeover itself rewrites the endpoint and token fields.
- Reports carry paths only, never values, because live files hold API keys.

`DriftService::resolve` takes one of three resolutions:
- `Reconcile` calls the new `ProxyService::repair_takeover_leftover_for_app`: restore with the backup → SSOT → placeholder-cleanup fallback, then delete the backup. The per-app `enabled` flag is kept, matching `stop_with_restore_keep_state`. Reconcile is refused while a takeover is live.
- `Fold { paths }` merges those live values into the app's common-config snippet (JSON deep-set, or `toml_edit` for Codex, which keeps comments). It marks implicit snippet users as explicit, the same way `set_common_config_snippet` does, enables common config on the current provider, and re-syncs live.
- `Ignore` stores the report's fingerprint (sha256 of paths, change kinds and current values) under `live_drift_ignored_<app>`. The same drift is no longer flagged; a changed value is a new drift.

**Watcher.** `services::drift::watcher` uses `notify` 8 to watch each app's config dir (non-recursive) and debounces events by 800 ms. It reacts only to files listed in the last report, with canonical paths added for macOS FSEvents, and pushes `live-config-drift` once per fingerprint. It starts at startup after crash recovery and proxy-state restore, runs on a blocking thread, and is kept alive in `DriftWatcherState`.

**Commands and frontend.** New commands: `get_live_config_drift`, `get_all_live_config_drift`, `resolve_live_config_drift`. The DTOs are exported to `src/shared/bindings.ts`. On the frontend:
- `driftApi` in `src/lib/api/drift.ts`.
- `useLiveConfigDrift`, mounted in `useAppShellEvents`, shows a persistent toast. Its main action is Repair or "Keep in common config", and its cancel action is Ignore.
- i18n keys added to all four locales, under `notifications.liveDrift*`.

**Files.**
- New: `src-tauri/src/services/drift/{mod,watcher,tests}.rs`, `src-tauri/src/commands/drift.rs`, `src/lib/api/drift.ts`, `src/hooks/useLiveConfigDrift.ts`, `tests/hooks/useLiveConfigDrift.test.ts`.
- Modified: `src-tauri/Cargo.toml` and `Cargo.lock` (adds `notify = "8"`), `src-tauri/src/{lib.rs,bindings.rs,commands/mod.rs,services/mod.rs}`, `src-tauri/src/services/provider/preview.rs` (`capture_live_write` is now `pub(crate)`), `src-tauri/src/services/proxy.rs` (adds `repair_takeover_leftover_for_app`; nothing under `src-tauri/src/proxy/` changed), `src/hooks/useAppShellEvents.ts`, `src/lib/api/index.ts`, `src/shared/bindings.ts` (regenerated), `src/i18n/locales/*.json`, `tests/msw/unmockedCommands.ts`.

**Commits.** 49c64f9 (code and tests). The ticket notes are in a follow-up commit.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib drift`: 12 passed (10 in `services::drift::tests`). These cover a simulated crash: takeover enabled, then the proxy server is stopped without restoring live. Detection reports the leftover placeholder and Reconcile restores the provider key and removes the backup. A legacy `PROXY_MANAGED` live config with no backup is rebuilt from SSOT. Reconcile is refused during a live takeover. An externally added `hooks` key is reported as `added`/foldable; a `*_API_KEY` is not foldable and its value never appears in the report. Fold writes the snippet, enables common config, and `hooks` survives a switch to another provider. Ignore/fingerprint, TOML/`.env` diffing, TOML fold that keeps comments, the tracker's notify-once behaviour, and a real inotify watch are also covered.
- `UPDATE_BINDINGS=1 cargo test --lib bindings`: 2 passed (bindings regenerated and up to date).
- `cargo fmt`: clean. `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` (full suite): 2642 passed, 0 failed.
- `pnpm typecheck`: clean. `pnpm test:unit`: 159 files, 1542 tests passed. `pnpm format:check`: clean.

**Not verified here.** The macOS FSEvents and Windows watcher backends compile through `notify`'s own cfg gates but were only run on Linux (inotify). During an active takeover, an external top-level key is still dropped when the takeover restores live from its backup. Folding it into the common config keeps it for later provider switches, but the backup itself is not changed.
