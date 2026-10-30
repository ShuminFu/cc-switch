# CCS-247: [terminal] Move the macOS AppleScript builders into an OS-neutral module so their tests run on Linux

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p3-low`, `area:terminal`

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
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib -- platform::terminal::applescript` reports 8 passed on Linux.
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --lib --tests -- -D warnings` and `cargo fmt --check` pass.

## Dependencies
None.
