# CCS-104: [terminal] Escape apostrophes in PowerShell launch paths

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:terminal`

> **Ticket key:** `p1-powershell-quote` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p0-hotfix-terminal-tempfiles`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-powershell-path-escape. `& '<bat path>'` is built without escaping. A profile such as O'Brien fails to parse, and the cmd fallback never runs because `start` succeeds.

## Evidence
- src-tauri/src/commands/misc.rs:3245 and :3484 (fallbacks at 3258-3266, 3496-3504)

## Proposed change
Add `powershell_single_quote(s)`, which doubles each ', and use it at both sites.

## Acceptance criteria / verification
- [x] Platform-independent unit test: `C:\Users\O'Brien\...\x.bat` gives `'C:\Users\O''Brien\...\x.bat'`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml powershell_single_quote`

## Dependencies
p0-hotfix-terminal-tempfiles (same file)

## Implementation notes
- **Summary:** Added `powershell_single_quote(value)` in `src-tauri/src/commands/misc.rs`, which wraps a value in single quotes and doubles each `'` (and the typographic single quotes U+2018/U+2019/U+201A/U+201B that PowerShell also treats as quote characters). Both Windows PowerShell launch sites (`launch_windows_terminal` and the Windows branch of `launch_terminal_running`) now build `& <quoted path>` with it instead of `& '{}'`. The helper is platform-independent (`allow(dead_code)` off Windows, like `escape_windows_batch_value`).
- **Files:** `src-tauri/src/commands/misc.rs`
- **Commits:** `07ee153` (code + tests); this ticket update follows in a separate docs commit.
- **Tests added** (`commands::misc::tests`):
  - `powershell_single_quote_doubles_apostrophes`: `C:\Users\O'Brien\AppData\Local\Temp\x.bat` gives `'C:\Users\O''Brien\AppData\Local\Temp\x.bat'`; plain paths and `$`/backtick/`&` stay as-is.
  - `powershell_single_quote_doubles_typographic_single_quotes`.
  - `powershell_single_quote_used_at_windows_launch_sites`: source-scan guard (both call sites are `cfg(windows)`) that fails on the old unescaped `format!("& '{}'", ...)` and requires both sites to use the helper.
- **Verification (Linux):**
  - `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean
  - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib powershell_single_quote`: 3 passed
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2296 passed, 0 failed, 2 ignored
- **Not verified locally:** I could not compile the `cfg(windows)` call sites. `cargo check --target x86_64-pc-windows-gnu` fails in `aws-lc-sys` because there is no `x86_64-w64-mingw32-gcc` toolchain here. The change there is a one-line substitution (`&Cow<str>` passed to `&str`), and the Windows CI build should confirm it. Nobody has tried a real launch from an apostrophe profile on Windows yet.
