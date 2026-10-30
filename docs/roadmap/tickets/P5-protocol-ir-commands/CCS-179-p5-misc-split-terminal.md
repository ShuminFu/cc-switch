# CCS-179: [commands] Split misc.rs into cli_tools services and a unified platform terminal stack

**Status:** Partial · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:commands`

> **Ticket key:** `p5-misc-split-terminal` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p1-powershell-quote`, `p0-hotfix-terminal-tempfiles`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- commands/misc.rs (~3.5k lines: search paths, version probes, installs, lifecycle, terminals); session_manager/terminal

## Proposed change
- services/cli_tools/{search_paths,version_probe,installations,lifecycle_cmd,remote_versions}.rs.
- platform/terminal/{macos,linux,windows}.rs, merged with session_manager/terminal and keeping the P0/P1 tempfile and quoting fixes.

## Acceptance criteria / verification
- [x] commands/misc.rs is deleted
- [ ] The terminal unit tests (tempfile paths, powershell quoting) pass on 3 OSes (Linux run natively; Windows run under Wine; macOS compiles and passes clippy but its tests still need a macOS CI run)
- [x] `cargo test --manifest-path src-tauri/Cargo.toml terminal cli_tools`

## Dependencies
p1-powershell-quote, p0-hotfix-terminal-tempfiles

## Implementation notes

**Summary.** `src-tauri/src/commands/misc.rs` (5.7k lines) is deleted. Its code moved, mostly verbatim, to these places:

- **`services/cli_tools/`** handles CLI tools:
  - `mod.rs`: `VALID_TOOLS`, `ToolLifecycleAction`, WSL distro lookup, and the async service functions `get_tool_versions`, `run_tool_lifecycle_action` and `probe_tool_installations`.
  - `search_paths.rs`: candidate install dirs and executable candidates.
  - `version_probe.rs`: `ToolVersion`, `ShellProbe`, PATH, scan and WSL probes.
  - `remote_versions.rs`: semver handling, npm dist-tags, GitHub and PyPI. A new `fetch_latest_version` holds the per-tool match that used to be inlined.
  - `installations.rs`: `ToolInstallation`, `ToolInstallationReport`, enumeration, `default_install` and `is_conflicting`.
  - `lifecycle_cmd.rs`: static and anchored install/update commands, the WSL wrapper, batch quoting, and the silent `bash -c` / `cmd /C` runner.
- **`platform/`** holds the shared helpers:
  - `process.rs`: `CREATE_NO_WINDOW` and `decode_command_output`.
  - `shell.rs`: `shell_single_quote`, `powershell_single_quote`, batch escaping and the cwd command, plus shell-name, flag and WSL-distro validation. These are pure functions, compiled and tested on every OS.
- **`platform/terminal/`** is the single terminal stack:
  - `mod.rs`: `resolve_launch_cwd`, `launch_terminal_with_env` and `launch_terminal_running`. Each one only dispatches to a per-OS module.
  - `tempfiles.rs`: the CCS-036 private temp files.
  - `posix.rs`: user-shell resolution and the provider and running launcher scripts that macOS and Linux share.
  - `macos.rs`, `linux.rs`, `windows.rs`: one module per OS, each cfg-gated.
  - `resume.rs`: the former `session_manager/terminal/mod.rs`. Its private `shell_escape` duplicate is replaced by the shared `shell_single_quote`. Its entry point is now `launch_resume_terminal`.
- The provider-terminal path and the trusted-command path (`launch_terminal_running`) used to repeat the preferred-terminal selection for each OS. Each OS now has one helper:
  - macOS: `launch_script_in_preferred_terminal`
  - Linux: `spawn_script_in_preferred_terminal`
  - Windows: `start_bat_in_preferred_terminal`

  Fallback order, error strings and failure cleanup are unchanged. The Linux existence check now tries the fixed bin dirs before `which` in both paths. The result is the same, but the trusted-command path used to call `which` first.
- **Tauri commands** keep their names and signatures. They moved to `commands/cli_tools.rs` (thin wrappers over the service), `commands/terminal.rs` (`open_provider_terminal` plus provider env extraction) and `commands/system.rs` (open_external, clipboard, updates, portable mode, init/migration results, window theme). `commands/hermes.rs` and `commands/session_manager.rs` now call `platform::terminal`. `docs/dev/ACCEPTANCE.md` now points at `services/cli_tools/` instead of `commands/misc.rs`.
- **Tests:**
  - All 124 tests from `misc.rs` and the 4 from `session_manager/terminal` moved with their code. The total is now 128 plus 1 new test.
  - The CCS-104 source scan now reads `terminal/windows.rs` and expects exactly one `format!("& {}", powershell_single_quote(&bat_path))`, because both Windows launch paths now go through `start_bat_in_preferred_terminal`.
  - New test: `platform::terminal::posix::tests::running_script_runs_command_then_waits_and_self_deletes`.

**Files:**
- Deleted: `src-tauri/src/commands/misc.rs`.
- Moved: `src-tauri/src/session_manager/terminal/mod.rs` → `src-tauri/src/platform/terminal/resume.rs`.
- New:
  - `src-tauri/src/commands/{cli_tools,system,terminal}.rs`
  - `src-tauri/src/services/cli_tools/{mod,search_paths,version_probe,installations,lifecycle_cmd,remote_versions}.rs`
  - `src-tauri/src/platform/{mod,process,shell}.rs`
  - `src-tauri/src/platform/terminal/{mod,posix,tempfiles,macos,linux,windows}.rs`
- Edited: `commands/{mod,hermes,session_manager}.rs`, `lib.rs`, `services/mod.rs`, `session_manager/mod.rs`, `docs/dev/ACCEPTANCE.md`.

**Commits:** 47e7ad9 (code). The follow-up commit records these notes.

**Verification (Linux x86_64):**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and `cargo clippy ... --tests -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml terminal cli_tools` is rejected by cargo, which takes only one test-name filter. The equivalent run is `cargo test --manifest-path src-tauri/Cargo.toml --lib -- terminal cli_tools platform::`: 94 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2771 passed, 0 failed, 3 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` (before the one new test was added): 3002 passed, 0 failed across all targets.
- No frontend or IPC surface changed: command names and argument shapes are the same, so no `pnpm` run was needed.

**Remaining (why Partial):**
- The `cfg(target_os = "macos")` and `cfg(target_os = "windows")` modules have not been compiled or tested here. That covers `platform/terminal/{macos,windows}.rs`, the Windows branches in `services/cli_tools/*`, and the Windows-only tests (`anchored_upgrade_windows`, `windows_helpers`, `wsl_helpers`) and macOS AppleScript tests.
  - The rust-std targets exist, but `cargo check --target x86_64-pc-windows-gnu` / `x86_64-apple-darwin` fail building `aws-lc-sys`, because there is no mingw/macOS SDK C toolchain (same limitation recorded in CCS-104).
  - I checked the imports of every cfg-gated item by hand against its new module (a script cross-referenced each identifier used in each file against its imports). A macOS and a Windows CI run are still needed before the "3 OSes" criterion can be ticked.
- The OS-independent terminal tests run on Linux: tempfile privacy and uniqueness, the launcher scripts, PowerShell and batch quoting, the cwd commands, and session-resume quoting.

**Review.** An independent review compared every function in the old `misc.rs` and `session_manager/terminal/mod.rs` with its new location, ignoring whitespace. Only the intended merges differ: the per-OS terminal selection helpers, `fetch_latest_version`, the thin command wrappers, and `shell_escape` replaced by `shell_single_quote`. All 12 Tauri commands and all 128 tests are still present. `scripts/check-ipc-orphans.mjs` reports 0 orphans. The cross-target builds that were missing before now ran:
- **Windows** (`x86_64-pc-windows-gnu`, with mingw-w64): `cargo clippy --lib --tests` found one warning this change introduced. The `shell_single_quote` import in the `tempfiles.rs` tests is used only by a `cfg(unix)` test, so it was unused on Windows. The fix gates the import with `#[cfg(unix)]`. The remaining Windows warnings are in files this ticket does not touch.
- **Windows tests under Wine**: the `terminal cli_tools platform::` filter gave 86 passed, 0 failed. These include `anchored_upgrade_windows`, `wsl_helpers` and the PowerShell and batch quoting tests. The full `--lib` run gave 2743 passed and 2 failed. Both failures (`config::tests::atomic_write_secret_sets_owner_only_acl` and `database::backup::tests::backup_commands_reject_colon_filenames_without_touching_files`) are in modules this ticket does not touch, and both depend on Wine's ACL and filename behaviour.
- **macOS** (`x86_64-apple-darwin`): with C build scripts stubbed out, which is enough for a check-only build, `cargo clippy --lib --tests` is clean for `platform/terminal/macos.rs` and the macOS branches. The macOS tests were not run, so the 3-OS item stays open until a macOS CI run.
- **Linux**: `cargo test --no-fail-fast` gave 3003 passed, 0 failed. `--lib` gave 2771 passed. fmt and clippy are clean.

### Remaining after triage

- Blocked: 'Terminal unit tests pass on 3 OSes' still needs a native macOS run. Needs the macos-latest CI leg or a Mac.
- Follow-up [CCS-247](../P5-protocol-ir-commands/CCS-247-p5-terminal-applescript-builders-neutral.md): Move the macOS AppleScript builders into an OS-neutral module so their tests run on Linux.
