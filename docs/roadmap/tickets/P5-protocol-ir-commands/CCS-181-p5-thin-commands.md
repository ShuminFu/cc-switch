# CCS-181: [commands] Make commands thin adapters over a Ctx/EventSink seam and reach 100% IpcError

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:commands`

> **Ticket key:** `p5-thin-commands` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p5-command-domains-permissions`, `p2-ipc-error-type`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- commands/proxy.rs:349-406 reset_circuit_breaker policy; commands/provider.rs:421-676 query_provider_usage_inner; *_internal/*_test_hook triplets

## Proposed change
- Add a Ctx/EventSink seam.
- Move the policy into a FailoverService and services/usage_query.rs.
- Delete the triplets.
- Add a CI check that fails on a command body over 40 lines or on direct state.db access.
- Migrate all remaining commands to IpcError.

## Acceptance criteria / verification
- [ ] No *_test_hook remains
- [ ] 100% of commands return IpcError (count script)
- [ ] The CI body-length check passes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`

## Dependencies
p5-command-domains-permissions, p2-ipc-error-type
