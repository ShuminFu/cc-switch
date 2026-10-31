# CCS-247: [terminal] Move the macOS AppleScript builders into an OS-neutral module so their tests run on Linux

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p3-low`, `area:terminal`

> **Ticket key:** `p5-terminal-applescript-builders-neutral` · **Phase:** P5 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-179, CCS-006). Source tickets: [CCS-179](../P5-protocol-ir-commands/CCS-179-p5-misc-split-terminal.md), [CCS-006](../P5-protocol-ir-commands/CCS-006-epic-p5.md).

## Problem
The 8 macOS terminal tests (`src-tauri/src/platform/terminal/macos.rs:310-472`) never run because `mod macos` is gated `#[cfg(target_os = "macos")]` (`platform/terminal/mod.rs:11`), yet they only exercise pure string builders: `applescript_string_literal` (71), `applescript_launcher_command` (76), `applescript_exec_launcher_command` (84), `build_macos_terminal_applescript` (94), `build_macos_iterm2_applescript` (143), `build_macos_dash_c_command` (184), `build_macos_ghostty_applescript` (195). Their only dependency, `shell_single_quote`, is OS-neutral.

## Proposed change
- Move these builders and their tests into `platform/terminal/applescript.rs`, declared `#[cfg(any(target_os = "macos", test))] mod applescript;` (or ungated with `#[cfg_attr(not(target_os = "macos"), allow(dead_code))]`).
- `macos.rs` keeps the launchers (`run_terminal_osascript`, `launch_macos_*`, `launch_script_in_preferred_terminal`) and imports the builders.
- If possible re-run the stubbed `x86_64-apple-darwin` clippy check from CCS-179's Review notes.
- Update CCS-179's notes: macOS test logic is covered on Linux; the 3-OS box still needs a native macOS run.

## Acceptance criteria / verification
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib -- platform::terminal::applescript` reports 8 passed on Linux.
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --lib --tests -- -D warnings` and `cargo fmt --check` pass.

## Dependencies
None.

## Implementation notes

**Summary.** The seven pure string builders (`applescript_string_literal`, `applescript_launcher_command`, `applescript_exec_launcher_command`, `build_macos_terminal_applescript`, `build_macos_iterm2_applescript`, `build_macos_dash_c_command`, `build_macos_ghostty_applescript`) and their 7 tests moved unchanged from `platform/terminal/macos.rs` to the new `platform/terminal/applescript.rs`. The module is declared `#[cfg(any(target_os = "macos", test))] mod applescript;`, so it is compiled on macOS and in every test build, and Linux release builds get no dead-code warnings. The four `build_*` functions are `pub(super)`. The three literal/launcher helpers stay private to the new module. `macos.rs` keeps `launch_provider_terminal`, `launch_running_terminal`, `launch_script_in_preferred_terminal`, `run_terminal_osascript` and the `launch_macos_*` launchers, and imports the four builders. It still imports `shell_single_quote` for the Warp launcher. One new test, `applescript_string_literal_escapes_backslashes_and_quotes`, checks backslash and double-quote escaping directly, which brings the module to the 8 tests the acceptance criterion expects.

**Files:**
- `src-tauri/src/platform/terminal/applescript.rs` (new)
- `src-tauri/src/platform/terminal/macos.rs`
- `src-tauri/src/platform/terminal/mod.rs`
- `docs/roadmap/tickets/P5-protocol-ir-commands/CCS-179-p5-misc-split-terminal.md` (notes)

**Commits:** 98337e0 (code). This ticket-file edit is in the follow-up docs commit.

**Verification (Linux):**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- platform::terminal::applescript`: 8 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2869 passed, 0 failed, 11 ignored.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib --tests -- -D warnings`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.

**Not re-run:** the stubbed `x86_64-apple-darwin` clippy check from CCS-179. It needs the C build scripts (`aws-lc-sys`) stubbed out and a separate target dir, and this host has about 3 GB of free disk. Instead, the `macos.rs` imports were checked by hand. Every remaining identifier (`shell_single_quote` for Warp, `decode_command_output`, `Path`, and the four `build_*` builders) is imported and used. In a macOS non-test build, every function in `applescript.rs` is reached from `macos.rs` either directly or through another builder, so no dead-code warning is expected there. The macos-latest CI leg will confirm this.
