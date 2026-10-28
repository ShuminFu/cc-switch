# CCS-153: [proxy] Split services/proxy.rs (7.2k lines) into lifecycle, takeover, backup, hot_switch and config modules

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:proxy`

> **Ticket key:** `p4-services-proxy-split` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-takeover-crash-e2e`, `p1-proxy-lifecycle-lock`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Move code without changing behaviour into services/proxy/{mod,lifecycle,takeover/{claude,codex,gemini,grok},backup,hot_switch,config}.rs.
- Keep the P1 lifecycle mutex in lifecycle.rs.

## Acceptance criteria / verification
- [ ] No file exceeds 1500 lines of production code
- [ ] Crash E2E, proxy_e2e and the services/proxy tests pass
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy`

## Dependencies
p4-takeover-crash-e2e, p1-proxy-lifecycle-lock
