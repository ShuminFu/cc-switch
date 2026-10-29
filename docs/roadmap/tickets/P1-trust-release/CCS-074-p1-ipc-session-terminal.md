# CCS-074: [ipc] Make launch_session_terminal build its resume command on the backend

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:ipc`

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
- [x] No #[tauri::command] parameter is used as a shell string (code review checklist in the PR)
- [x] Test: a session id containing `;rm -rf` is quoted or rejected
- [x] `cargo test --manifest-path src-tauri/Cargo.toml session_manager`, `pnpm typecheck`, `pnpm test:unit`

## Dependencies
None.

## Implementation notes

**Summary.** `launch_session_terminal` now accepts only `{providerId, sessionId}`; the
`command`, `cwd` and `custom_config` parameters are gone. In a blocking task the backend
calls `session_manager::resolve_resume_target`, which:
- rejects session ids that are empty, start with `-` (so the CLI cannot read them as an
  option) or contain control characters;
- scans only the given provider's sessions and requires an exact
  `{provider_id, session_id}` match (unknown sessions are rejected; `openclaw`/`hermes`/unknown
  providers are rejected because they have no CLI resume);
- builds the command with `build_resume_command` from fixed templates
  (`claude --resume`, `codex resume`, `gemini --resume`, `grok --resume`, `opencode -s`).
  The session id goes in as one argument: ids made only of `[A-Za-z0-9._-]` are left bare (so
  displayed commands look the same as before), and every other id is passed through
  `shell_single_quote`;
- takes `cwd` from the scanned session's `project_dir` instead of from the webview.

The provider scanners now use the same `build_resume_command`, so the copy-to-clipboard
fallback and the displayed command are quoted the same way. The terminal module's
`shell_escape` (used for `cd <cwd> &&`) switched from double quotes to POSIX single quotes,
because `$(...)` and backticks still expand inside double quotes. The `custom` terminal target
now always gets `None` for its template. No caller ever passed one, so behaviour is unchanged,
and a webview-supplied template would have been a raw `sh -c` string.

Review checklist, "No #[tauri::command] parameter is used as a shell string": I checked every
parameter of the commands in `commands/session_manager.rs`. `providerId`/`sessionId` are only
used for matching, and a quoted copy of the matched id is used for the template. The
`get_session_messages` and `delete_*` path parameters are never passed to a shell.

TS callers (`grep -rn launchTerminal src`; the wrapper is named `sessionsApi.launchTerminal`):
`src/lib/api/sessions.ts` and `src/components/sessions/SessionManagerPage.tsx` (`handleResume`).

**Files.** `src-tauri/src/commands/session_manager.rs`, `src-tauri/src/session_manager/mod.rs`,
`src-tauri/src/session_manager/terminal/mod.rs`,
`src-tauri/src/session_manager/providers/{claude,codex,gemini,grokbuild,opencode}.rs`,
`src/lib/api/sessions.ts`, `src/components/sessions/SessionManagerPage.tsx`,
`tests/lib/sessionsApi.test.ts` (new).

**Commits.** `575057e` (code + tests); the ticket-notes commit follows it.

**Tests added.**
- `session_manager::tests::resume_command_single_quotes_shell_metacharacters_in_session_id`:
  `abc;rm -rf ~` becomes `claude --resume 'abc;rm -rf ~'`, and embedded `'` / `$(id)` are escaped.
- `resume_target_quotes_malicious_session_id_and_uses_backend_cwd`,
  `resume_target_rejects_unknown_or_unsafe_session_ids`,
  `resolve_resume_target_rejects_providers_without_cli_resume`,
  `resume_command_uses_fixed_templates_for_plain_ids`.
- `terminal::tests::shell_escape_neutralizes_command_substitution_in_cwd`: fails on the old
  double-quote escaper and passes now.
- `tests/lib/sessionsApi.test.ts`: the wrapper sends only `providerId` and `sessionId`.

**Verification (Linux).**
- `cargo fmt`: clean. `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib session_manager`: 71 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2190 passed, 0 failed, 3 ignored.
- `pnpm typecheck`: clean. `pnpm test:unit`: 126 files, 1277 tests passed.
  `prettier --check` on the touched TS files: clean.
- Not tested: actually launching a macOS terminal (`launch_terminal` returns an error on
  non-macOS), so a macOS check of the Resume button is still needed.
