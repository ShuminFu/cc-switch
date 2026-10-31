# CCS-083: [auth] Harden the Copilot and Codex OAuth token stores

**Status:** Partial · **Kind:** security · **Labels:** `security`, `phase:P1`, `p2-medium`, `area:auth`

> **Ticket key:** `p1-oauth-token-store-hardening` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p1-atomic-write-perms`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The critique found that copilot_auth.rs (2083 lines) and codex_oauth_auth.rs (1127 lines) keep plaintext token JSON, each with its own atomic writer. They have no Windows equivalent of 0600, no fsync, and are not covered by the lint or the plan's catalog.

## Evidence
- src-tauri/src/proxy/providers/copilot_auth.rs:1247-1294 (:1273 0600); codex_oauth_auth.rs:811

## Proposed change
- Replace both writers with a shared `atomic_write_secret` (0600, fsync, symlink-safe).
- On Windows, set an owner-only ACL on the file.
- Add the token files to the config-writer snapshots.
- The keychain migration is tracked in p4-keychain-secrets.

## Acceptance criteria / verification
- [x] Unix test: the token file is 0600 after first write and after rewrite
- [ ] Windows CI test: the ACL has only the current user
- [x] `cargo test --manifest-path src-tauri/Cargo.toml copilot_auth codex_oauth`

## Dependencies
p1-atomic-write-perms

## Implementation notes

**Summary.** Added `config::atomic_write_secret` next to `atomic_write`. It
shares the same path (temp file + `sync_all`, rename with Windows retry,
parent-dir fsync on unix, temp cleanup on any error, writes through
symlinks without replacing them) but never inherits the target's mode:

- unix: the temp file is forced to 0600 before rename, so a token file that
  was loosened externally (e.g. 0644) is tightened again on every save;
- Windows: before the rename the temp file gets a protected DACL
  (`SetEntriesInAclW` + `SetNamedSecurityInfoW` with
  `PROTECTED_DACL_SECURITY_INFORMATION`) holding a single `FILE_ALL_ACCESS`
  ACE for the current process user's SID, no inherited ACEs. A same-volume
  rename keeps the security descriptor. Failure to set the ACL fails the
  write (fail closed, like the unix `set_permissions` error).

`CopilotAuthManager::write_store_atomic` and
`CodexOAuthManager::write_store_atomic` now delegate to it (previously no
fsync, and on Windows a delete-then-rename with the inherited ACL). Both
files no longer use `OpenOptions`, so they were removed from the
`tests/write_lint.rs` allowlist. The token stores were already in the
config-writer snapshots (`tests/config_writers.rs`: `copilot_token_store`,
`codex_oauth_token_store`, `written_file_modes`), which pass unchanged.
Keychain migration stays in p4-keychain-secrets.

**Files.** `src-tauri/Cargo.toml` (windows-sys features `Win32_Foundation`,
`Win32_Security`, `Win32_Security_Authorization`, `Win32_Storage_FileSystem`,
`Win32_System_Threading`), `src-tauri/src/config.rs`,
`src-tauri/src/proxy/providers/copilot_auth.rs`,
`src-tauri/src/proxy/providers/codex_oauth_auth.rs`,
`src-tauri/tests/write_lint.rs`.

**Tests.**
- `config::tests::atomic_write_secret_forces_owner_only_on_create_and_rewrite`
  (unix; a 0644 target becomes 0600, which `atomic_write` would keep as 0644)
- `config::tests::atomic_write_secret_keeps_symlink_and_restricts_target` (unix)
- `config::tests::atomic_write_secret_sets_owner_only_acl` (`#[cfg(windows)]`;
  reads the DACL back and asserts it is protected and has exactly one
  ACCESS_ALLOWED ACE whose SID equals the current user, after the first write
  and after a rewrite)
- `copilot_auth::tests::test_token_store_is_owner_only_after_first_write_and_rewrite`
  and `codex_oauth_auth::tests::test_token_store_is_owner_only_after_first_write_and_rewrite` (unix)

**Commits.** 5008628 (code + tests); this notes edit in a follow-up commit.

**Verification (Linux).**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- copilot_auth codex_oauth atomic_write`: 74 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --test write_lint --test config_writers`: 3 + 17 passed
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2390 passed, 0 failed
- Windows code: the full crate cannot be cross-checked here (`aws-lc-sys` needs
  a mingw C toolchain), so the `owner_only_acl` module and the Windows test
  helper were extracted verbatim into a scratch crate against
  `windows-sys 0.61.2` and `cargo +1.95 check/clippy --target
  x86_64-pc-windows-gnu --tests -- -D warnings` passed.

**Remaining.** The Windows ACL test has not been executed: it runs in the
existing `windows-latest` job of `.github/workflows/ci.yml` (`cargo test`).
Mark the "Windows CI test" item done once that job passes.

### Remaining after triage

- Blocked: Windows CI test `config::tests::atomic_write_secret_sets_owner_only_acl`. Needs a Windows machine or the windows-latest backend matrix job.
- Follow-ups: none; nothing left here can be coded or verified locally.

### Wine evidence: full Windows suite (2026-10-31)

- Run: `cargo test --target x86_64-pc-windows-gnu --no-fail-fast` under Wine 9.0 (mingw cross-compile, `LANG=LC_ALL=C.UTF-8`, `WINEPATH` set to the target dir), at `bdd666e`. Result: 3092 passed, 1 failed, 3 ignored. The one failure is `config::tests::atomic_write_secret_sets_owner_only_acl`.
- That failure is a Wine limitation, not a product bug. A minimal mingw C repro (`SetEntriesInAclW` + `SetNamedSecurityInfoW(DACL | PROTECTED_DACL)` on a new file, then `GetNamedSecurityInfoW`) shows that Wine returns `ERROR_SUCCESS` but does not keep the DACL. Reading it back gives a DACL with 3 ACEs synthesised from the unix mode, control `0x8004` (no `SE_DACL_PROTECTED`), and no `user.wine.sd` xattr on the file. `GetKernelObjectSecurity` gives the same result. The test was not changed and must still pass on real Windows.
- The rest of the `#[cfg(windows)]` atomic-write path passed under Wine: `rename_replace` overwrites without `remove_file`, there are no leftover `*.tmp.*` files, and creating then rewriting through `atomic_write_secret` works.
- Still needs real Windows: the owner-only DACL assertion (a windows-latest CI job or a Windows machine).
