# CCS-074: [ipc] Make launch_session_terminal build its resume command on the backend

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:ipc`

> **Ticket key:** `p1-ipc-session-terminal` · **Phase:** P1 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
launch_session_terminal accepts a free-form command string from the webview and runs it in a terminal. A renderer compromise therefore leads directly to shell execution. Per the critique, bindings do not exist yet, so callers must be inventoried by grep.

## Evidence
- src-tauri/src/commands/session_manager.rs:28; frontend callers under src/components/sessions/** and src/lib/api

## Proposed change
- Change the signature to `{provider_id, session_id}`.
- The backend looks up session metadata and builds the resume command from fixed templates, using shell_single_quote for arguments.
- Update the TS wrapper and every caller found by `grep -rn launchSessionTerminal src`.

## Acceptance criteria / verification
- [ ] No #[tauri::command] parameter is used as a shell string (code review checklist in the PR)
- [ ] Test: a session id containing `;rm -rf` is quoted or rejected
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml session_manager`, `pnpm typecheck`, `pnpm test:unit`

## Dependencies
None.
