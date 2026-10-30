# CCS-036: [terminal] Write provider terminal config and launcher scripts as private, uniquely named temp files

**Status:** Partial · **Kind:** security · **Labels:** `security`, `phase:P0`, `p1-high`, `area:terminal`

> **Ticket key:** `p0-hotfix-terminal-tempfiles` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Covers verified findings backend-terminal-tmp-key and backend-terminal-script-name-collision.
- On Linux the provider API key is written to a predictable /tmp/claude_<id>_<pid>.json with 0644 and no O_EXCL. The cleanup trap never fires because the script ends with exec, so the file stays until reboot.
- Launcher script names depend only on the PID, so quick consecutive launches run the wrong provider's script or lose it to another session's cleanup.

## Evidence
- (Historical, before the CCS-179/181 split; see "Evidence refresh (CCS-221)" below for the current locations.) src-tauri/src/commands/misc.rs:2724-2729 (path), 2757-2773 (std::fs::write), 3098-3160 (Linux launcher + exec at build_exec_line :1125), 2789 (macOS), 3122 (Linux), 3223 (Windows .bat), 3345 (launch_terminal_running)

## Proposed change
- Create the files with tempfile::Builder (0600, random suffix, create_new) under app_config_dir/tmp, or $XDG_RUNTIME_DIR where available, and keep() them.
- Sanitize the provider id.
- Remove the config file after claude exits, before the exec line.

## Acceptance criteria / verification
- [x] Unit test: the helper returns distinct paths across calls, and on unix mode & 0o077 == 0
- [x] Manual (Linux): `ls -l` shows -rw-------; the file is gone after exiting claude; A then B quickly each print their own config path (verified headless with xterm under xvfb-run and a stub claude, see "Linux verification (CCS-223)")
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib platform::terminal` and clippy (was `commands::misc` before the code moved in CCS-179/181)

## Dependencies
None.

## Implementation notes
**Summary.** All terminal-launch temp files in `src-tauri/src/commands/misc.rs` now go through a new
`write_private_temp_file` helper (`tempfile::Builder`, 12 random bytes in the name, `O_EXCL`,
mode 0600, or 0700 for scripts, then `keep()`).
- `private_launch_temp_dir()`: on Linux, `$XDG_RUNTIME_DIR` if it is set to an absolute directory,
  otherwise `<app_config_dir>/tmp`, created with mode 0700. On macOS and Windows it uses the system
  temp dir, which is already per-user (`$TMPDIR` / `%TEMP%`).
- `sanitize_temp_file_component()`: keeps only `[A-Za-z0-9_-]`, up to 32 characters, and falls back
  to `provider`. It is applied to the provider id and to the `launch_terminal_running` label.
- `write_claude_config()` now creates the file itself and returns its path.
- `build_provider_launcher_script()` (macOS and Linux): this is now the only place the launcher
  script text is built.
  - It keeps the config path in a single-quoted shell variable.
  - It runs `rm -f "$cc_switch_config" "$0"` after `claude` exits and before the final `exec`. An
    EXIT trap does not run on `exec`, so that line is what removes the file.
  - The trap is kept as a fallback for early exit.
- The Windows provider `.bat` and the macOS, Linux and Windows scripts written by
  `launch_terminal_running` now get unique names.
- If writing the launcher script fails, the config file is removed.
- Out of scope, unchanged: the synchronous Windows silent-exec `.bat` (`cc_switch_<label>_<pid>.bat`,
  around line 233). It is written, run and deleted inside a single call.

**Files:** `src-tauri/src/commands/misc.rs` (code + tests in `tests::private_temp_files`).

**Commits:** d7b5297 (code), plus the follow-up commit that records these notes.

**Verification (Linux):**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib commands::misc`: 79 passed, including 6 new tests:
  - distinct paths, with `mode & 0o077 == 0`
  - scripts get mode 0o700
  - config files are private and unique, with sanitized names
  - sanitizer cases
  - `rm` comes before `exec` in the script
  - regression test: runs the generated script with `sh` and checks that the config file and the script are gone even though it ends in `exec`
- The regression was confirmed by hand. A script with the old shape (`trap ... EXIT` + `exec true`) leaves the config file on disk.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2016 passed, 0 failed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo fmt`: clean.

**Remaining (why Partial):**
- The manual desktop check from the acceptance criteria has not been run. It needs a graphical
  terminal session, which this headless container does not have:
  - `ls -l` shows `-rw-------`
  - the file is gone after exiting claude
  - launching A and then B quickly shows each one its own config path
- The cfg-gated macOS and Windows branches were reviewed but not compiled. Only the
  `x86_64-unknown-linux-gnu` target is installed here, so a macOS/Windows CI build should confirm them.

### Remaining after triage

- Blocked: Compile and run the cfg-gated macOS/Windows branches (platform/terminal/macos.rs, windows.rs). Needs a windows-latest/macos-latest run of the ci.yml backend matrix, or real machines (cross-compiling to windows-gnu/apple fails on aws-lc-sys).
- Blocked: Confirmation on a real desktop terminal (gnome-terminal/konsole with the real claude CLI). Needs interactive desktop use; CCS-223 covers the Linux behaviour.
- Follow-up [CCS-221](../P0-safety-net/CCS-221-p0-docs-ticket-evidence-refresh.md): Tick already-verified acceptance boxes and refresh stale evidence in Partial tickets.
- Follow-up [CCS-223](../P0-safety-net/CCS-223-p0-terminal-tempfile-headless-e2e.md): Headless Linux end-to-end check of private terminal temp files, plus temp-dir selection tests.

### Evidence refresh (CCS-221)

The terminal launch code moved out of `commands/misc.rs` (CCS-179/181). Current
locations:
- `src-tauri/src/platform/terminal/tempfiles.rs`: `write_claude_config`,
  `private_launch_temp_dir`, `sanitize_temp_file_component`,
  `write_private_temp_file`, and the tests in `tests::private_temp_files`
  (distinct private paths, 0o700 scripts, private unique config files,
  sanitizer, `rm` before `exec`, and the `sh` regression test that the config
  and script are gone after an `exec`-terminated script).
- `src-tauri/src/platform/terminal/posix.rs`: `build_exec_line`,
  `build_provider_launcher_script`, `write_provider_launcher`,
  `build_running_script`.
- `src-tauri/src/platform/terminal/linux.rs`: `launch_provider_terminal`,
  `launch_running_terminal`; `src-tauri/src/platform/terminal/mod.rs`:
  `launch_terminal_running`. macOS and Windows branches are in `macos.rs` and
  `windows.rs`.

Re-run on the lane2/followups tree at f13a403 (Linux):
- `cargo test --manifest-path src-tauri/Cargo.toml --lib platform::terminal`
  → 24 passed, 0 failed (includes the 6 `private_temp_files` tests).
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
  → clean.
- The unit-test and test+clippy boxes are ticked. The manual desktop check and
  the macOS/Windows compile stay open (see "Remaining after triage").

### Linux verification (CCS-223)

`ccs036_e2e` (ignored by default) in `src-tauri/src/platform/terminal/tempfiles.rs`
runs the real `launch_terminal_with_env` path with `preferred_terminal = "xterm"`,
`SHELL=/bin/sh` and a stub `claude` first on `PATH`, launching provider A and then
provider B straight away. It runs once with a 0700 `XDG_RUNTIME_DIR` and once with it
unset (fallback `<app_config_dir>/tmp`). Run on lane2/followups at a2ec202:

```
$ xvfb-run -a cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored ccs036_e2e --nocapture
ccs036_e2e provA: path: "/tmp/.tmpQlSlC1/claude_provA_8Qhl4Lab7sfU.json", mode: "600", ls: "-rw------- 1 root root 104 ... claude_provA_8Qhl4Lab7sfU.json", launcher_modes: ["700", "700"], ANTHROPIC_AUTH_TOKEN: "sk-provA"
ccs036_e2e provB: path: "/tmp/.tmpQlSlC1/claude_provB_IVG400Or0jgl.json", mode: "600", ls: "-rw------- 1 root root 104 ... claude_provB_IVG400Or0jgl.json", launcher_modes: ["700", "700"], ANTHROPIC_AUTH_TOKEN: "sk-provB"
test platform::terminal::tempfiles::tests::ccs036_e2e::ccs036_e2e_xdg_runtime_dir ... ok
ccs036_e2e provA: path: "/tmp/.tmp6PKUhH/.cc-switch/tmp/claude_provA_3AVGB6touD7v.json", mode: "600", ls: "-rw------- ...", ANTHROPIC_AUTH_TOKEN: "sk-provA"
ccs036_e2e provB: path: "/tmp/.tmp6PKUhH/.cc-switch/tmp/claude_provB_Y6eWlI66eA9R.json", mode: "600", ls: "-rw------- ...", ANTHROPIC_AUTH_TOKEN: "sk-provB"
test platform::terminal::tempfiles::tests::ccs036_e2e::ccs036_e2e_app_config_fallback ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 2857 filtered out; finished in 2.40s
```

Each launch got its own config path holding its own provider's env, mode 600
(`-rw-------`), launchers were 0700, and no `claude_*` config or
`cc_switch_launcher_*.sh` was left once the stub exited. The Linux manual box is ticked
on this evidence. A run with gnome-terminal/konsole and the real claude CLI was not done,
and the macOS/Windows compile is still open, so the status stays Partial.
