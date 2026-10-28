# CCS-036: [terminal] Write provider terminal config and launcher scripts as private, uniquely named temp files

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P0`, `p1-high`, `area:terminal`

> **Ticket key:** `p0-hotfix-terminal-tempfiles` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Covers verified findings backend-terminal-tmp-key and backend-terminal-script-name-collision.
- On Linux the provider API key is written to a predictable /tmp/claude_<id>_<pid>.json with 0644 and no O_EXCL. The cleanup trap never fires because the script ends with exec, so the file stays until reboot.
- Launcher script names depend only on the PID, so quick consecutive launches run the wrong provider's script or lose it to another session's cleanup.

## Evidence
- src-tauri/src/commands/misc.rs:2724-2729 (path), 2757-2773 (std::fs::write), 3098-3160 (Linux launcher + exec at build_exec_line :1125), 2789 (macOS), 3122 (Linux), 3223 (Windows .bat), 3345 (launch_terminal_running)

## Proposed change
- Create the files with tempfile::Builder (0600, random suffix, create_new) under app_config_dir/tmp, or $XDG_RUNTIME_DIR where available, and keep() them.
- Sanitize the provider id.
- Remove the config file after claude exits, before the exec line.

## Acceptance criteria / verification
- [ ] Unit test: the helper returns distinct paths across calls, and on unix mode & 0o077 == 0
- [ ] Manual (Linux): `ls -l` shows -rw-------; the file is gone after exiting claude; A then B quickly each print their own config path
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml commands::misc` and clippy

## Dependencies
None.
