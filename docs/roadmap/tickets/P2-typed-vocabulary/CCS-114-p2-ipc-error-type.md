# CCS-114: [ipc] Introduce IpcError with stable error codes

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p2-medium`, `area:ipc`

> **Ticket key:** `p2-ipc-error-type` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Commands return String errors, and the frontend matches Chinese substrings.

## Evidence
- error.rs:125 format_skill_error; AppError::Localized

## Proposed change
- Add `IpcError{code: ErrorCode, message, details, i18n_key}` with From<AppError> (keeping Localized.key) and a temporary From<String> that maps to UNKNOWN.
- Replace format_skill_error.
- Migrate the skills, proxy and provider domains first.

## Acceptance criteria / verification
- [ ] Serialization test for every ErrorCode
- [ ] A count script reports at least 50% of commands returning IpcError
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml error`

## Dependencies
None.
