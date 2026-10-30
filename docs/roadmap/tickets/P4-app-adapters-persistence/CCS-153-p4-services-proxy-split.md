# CCS-153: [proxy] Split services/proxy.rs (7.2k lines) into lifecycle, takeover, backup, hot_switch and config modules

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:proxy`

> **Ticket key:** `p4-services-proxy-split` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-takeover-crash-e2e`, `p1-proxy-lifecycle-lock`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Move code without changing behaviour into services/proxy/{mod,lifecycle,takeover/{claude,codex,gemini,grok},backup,hot_switch,config}.rs.
- Keep the P1 lifecycle mutex in lifecycle.rs.

## Acceptance criteria / verification
- [x] No file exceeds 1500 lines of production code
- [x] Crash E2E, proxy_e2e and the services/proxy tests pass
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy`

## Dependencies
p4-takeover-crash-e2e, p1-proxy-lifecycle-lock

## Implementation notes

**Summary.** `services/proxy.rs` (7,752 lines: about 3,450 production, 4,300 tests) is now the `services/proxy/` module. Every `ProxyService` method was moved verbatim into one `impl ProxyService` block per submodule. Before formatting, each moved method body was compared token for token against the original, with whitespace and trailing commas ignored, and the test module was compared the same way. Helpers that used to be private are now `pub(in crate::services::proxy)`, so the sibling modules and the tests can reach them. Nothing outside the module changed visibility. The P1 lifecycle mutex (`LifecycleMutex`) is defined in `lifecycle.rs`, and the `start`/`stop`/`start_locked`/`stop_locked` paths that take it live there too.

| File | Lines | Contents |
| --- | --- | --- |
| `services/proxy/mod.rs` | 121 | `ProxyService` struct, `HotSwitchOutcome`, `PROXY_TOKEN_PLACEHOLDER`, `new`, current-provider lookups, switch locks, `bind_proxy_token` |
| `services/proxy/lifecycle.rs` | 401 | lifecycle mutex, start / start_with_takeover / stop / stop_with_restore*, crash recovery, status |
| `services/proxy/config.rs` | 312 | proxy URLs, get/update config, restart_with_config, circuit breaker |
| `services/proxy/backup.rs` | 584 | live backups, restore (with fallback/SSOT), backup refresh from provider |
| `services/proxy/hot_switch.rs` | 378 | hot switch, rollback, live sync while the proxy is active |
| `services/proxy/takeover/mod.rs` | 976 | takeover toggles/status, live->provider sync, takeover write, detection, placeholder cleanup |
| `services/proxy/takeover/{claude,codex,gemini,grok}.rs` | 394 / 347 / 66 / 59 | per-app takeover fields and live read/write |
| `services/proxy/tests.rs` | 4,304 | existing tests (unchanged) plus `proxy_service_modules_stay_under_line_budget` |

Other edits: `apps/registry.rs` `former_hard_coded_app_lists_stay_removed` now runs its grep over all files in the new module, using `concat!` of the `include_str!`s. The comment in `apps/mod.rs` now points at `services/proxy/`.

Not touched, to stay out of the other lane: there are stale `services/proxy.rs` mentions in `src/components/proxy/ProxyPanel.tsx` (a comment), `docs/dev/ACCEPTANCE.md`, `CHANGELOG.md` and older ticket docs.

**Commits.** fc62469 (code), plus this ticket-notes commit.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 3,011 passed, 0 failed across 25 test binaries. The lib suite was 2,774 passed. `tests/proxy_e2e.rs` 32 passed. `tests/takeover_crash.rs` 5 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml proxy`: 1,536 passed, 0 failed.
- `cargo test --lib services::proxy`: 63 passed (62 existing tests plus the new line-budget test).
