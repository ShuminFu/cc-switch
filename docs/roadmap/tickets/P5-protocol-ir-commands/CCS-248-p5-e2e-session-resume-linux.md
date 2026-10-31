# CCS-248: [e2e] Automate the Linux session-resume check under xvfb with a fake claude

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p3-low`, `area:e2e`

> **Ticket key:** `p5-e2e-session-resume-linux` · **Phase:** P5 · **Priority:** p3-low · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-184, CCS-006). Source tickets: [CCS-184](../P5-protocol-ir-commands/CCS-184-p5-session-resume-cross-platform.md), [CCS-006](../P5-protocol-ir-commands/CCS-006-epic-p5.md).

## Problem
CCS-184's Linux manual resume check (Resume opens a terminal in the session cwd running `claude --resume <id>`, the temp script is deleted, the window stays open) can be automated: Xvfb, xvfb-run, xterm, WebKitWebDriver, tauri-driver and a debug binary are all available, and `e2e/wdio.conf.mjs` already isolates HOME (`CC_SWITCH_E2E_HOME`).

## Proposed change
- Add `e2e/specs/session-resume.e2e.mjs`. Before launch: write a fake Claude session JSONL under `$HOME/.claude/projects/<encoded-dir>/<id>.jsonl` whose cwd is a temp dir with a space and parentheses in its name; put a fake `claude` first on the app's PATH (via `appEnv`) that records `pwd` and its args to a marker file; seed `preferred_terminal = "xterm"` (`linux.rs:69` falls back to `-e`).
- Spec: open Sessions, select the session, click Resume, wait for the marker. Assert cwd == session dir, args exactly `--resume <id>`, `cc_switch_resume_*.sh` deleted, no error toast, xterm still alive (exec $SHELL).
- Non-GUI alternative: an `#[ignore]` Rust test calling `platform::terminal::launch_resume_terminal` under xvfb-run with the same fake claude.

## Acceptance criteria / verification
- [x] `xvfb-run -a pnpm --dir e2e test` passes with the new spec (after `pnpm tauri build --debug --no-bundle`).
- [x] Result recorded in CCS-184 as the automated Linux check.

## Dependencies
None.

## Implementation notes

**Summary.** New WebdriverIO spec `e2e/specs/session-resume.e2e.mjs`, run by the existing tauri-driver suite against the real debug binary:
- Setup, done in the spec at runtime. Settings are kept in memory, so `preferredTerminal = "xterm"` is set through `get_settings`/`save_settings`. xterm is not in the built-in Linux list, so `linux.rs` launches it with `-e`. The spec writes a Claude session JSONL under `$HOME/.claude/projects/<encoded cwd>/<uuid>.jsonl` whose `cwd` is a `mkdtemp` dir named `ccs248 resume (dir) XXXXXX`. It also writes a fake `claude` into `CC_SWITCH_E2E_BIN`. The fake answers `--version` (so version probes never count as a resume) and otherwise writes `pwd -P` and each argument to a marker file.
- Steps: open Session Manager, select the seeded session (checks that the resume-command preview reads `claude --resume <id>`), click **Resume Session**, then wait for the marker.
- Assertions:
  - the marker is exactly `<realpath session dir>\n[--resume]\n[<id>]\n`, so the cwd keeps its space and parentheses and the id is a single argument;
  - exactly one new xterm process appeared;
  - the "Terminal launched" toast is shown and there is no `data-type="error"` sonner toast;
  - the fake finds the `cc_switch_resume_*.sh` launch script among its ancestors (`sh <script>`), which must sit in `<HOME>/.cc-switch/tmp` or an absolute `$XDG_RUNTIME_DIR` and still exist while the resume command runs; that exact file is gone afterwards;
  - after 2 s, xterm is still alive and has exactly one child, the user shell from the `exec` line. That shell's `/proc/<pid>/cwd` is the session dir and its cmdline no longer mentions the launch script.
- Cleanup in `after`: kills the xterm, goes back to the main view (the app reopens on the last view, which broke the next spec on the first full run), restores the previous `preferredTerminal`, and removes the fake, the marker, the session log and the temp dir.
- `e2e/wdio.conf.mjs`: new `CC_SWITCH_E2E_BIN` (default `<e2e HOME>/bin`), prepended to the app's `PATH` in `appEnv()`. It is empty unless a spec puts a stub there.
- `.github/workflows/ci.yml` and `release.yml`: the e2e system-deps step also installs `xterm`.
- The ticket offered an `#[ignore]` Rust test calling `launch_resume_terminal` as a non-GUI alternative. It was not needed because the GUI path runs here. The Rust side is already covered by `posix_resume_script_executes_resume_in_session_dir_and_self_deletes` and the CCS-223 `ccs036_e2e` xterm test.

**Files:** `e2e/specs/session-resume.e2e.mjs` (new), `e2e/wdio.conf.mjs`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, this ticket, `CCS-184-p5-session-resume-cross-platform.md` (notes).

**Commits:** a14a45e (spec, config, workflows). This ticket edit and the CCS-184 record are in the follow-up docs commit.

**Verification (Linux, Xvfb, XTerm(390), tauri-driver, WebKitWebDriver):**
- `pnpm tauri build --debug --no-bundle`: built `src-tauri/target/debug/cc-switch`.
- `xvfb-run -a pnpm --dir e2e test --spec ./specs/session-resume.e2e.mjs` on a fresh HOME: 3 passing. The run printed `xterm <pid> -> bash (<pid>)`.
- `xvfb-run -a pnpm --dir e2e test` (full suite): 4 spec files passed (smoke 6, csp 6, session-resume 3, session-sync-responsiveness 2). No xterm, tauri-driver or WebKitWebDriver process and no `/tmp/ccs248*` dir was left behind.
- Negative control: the `{exec_line}` line was removed from `build_posix_resume_script` and the binary rebuilt. The spec then failed in "resumes the session in xterm in the session directory" because xterm had already exited (`Expected length: 1, Received length: 0`). The source was restored and the binary rebuilt before the full-suite run.
- `actionlint .github/workflows/ci.yml .github/workflows/release.yml`: clean. Both files also load with `yaml.safe_load`.
- `pnpm typecheck`: clean. `pnpm test:unit`: 173 files / 1668 tests passed. `pnpm format:check` and `prettier --check` on the e2e files: clean.
- No Rust code changed, so the cargo suites were not re-run.

**Review.** The original deletion check only listed `cc_switch_resume_*.sh` files in the expected dirs, so it passed vacuously if the script landed anywhere else (for example the `env::temp_dir()` fallback) and could fail on unrelated leftovers in a shared `$XDG_RUNTIME_DIR`. The fake `claude` now walks its ancestors' `/proc/<pid>/cmdline` for the launch script path and records whether the file exists at that point; the spec asserts the path is in the private launch dir, existed during the run and is deleted afterwards. Re-verified: the spec passes with `XDG_RUNTIME_DIR` unset (script under `<HOME>/.cc-switch/tmp`) and set to a 0700 temp dir (script there); the full `xvfb-run -a pnpm --dir e2e test` passes (4 spec files: 6, 6, 3, 2); `pnpm format:check` clean; no xterm, tauri-driver, WebKitWebDriver or `/tmp/ccs248*` left behind.

**Not covered:** real gnome-terminal and konsole on an interactive desktop, which still use `--` and `-e` from the built-in list. The CI legs pick up the spec once they run with the xterm install. They were not run from here.
