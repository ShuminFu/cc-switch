# CCS-006: [epic] P5 (v4.0): Canonical protocol IR and codecs, plus command-layer cleanup

**Status:** Partial · **Kind:** epic · **Labels:** `epic`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `epic-p5` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** XL · **Depends on:** `epic-p4`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal

Fix the root cause of bridge drift by expressing every bridge as decode(upstream) -> IR -> encode(client), one bridge per release candidate. Finish extracting the command layer and the app shell, including per-domain capability permissions (moved here by the critique, after collect_commands!).

## Exit criteria

- [ ] All 5 bridges run on codecs; the old providers/transform*.rs and streaming*.rs files are deleted; net reduction of at least 8k lines; fixtures pass or the diffs are documented
  - Audit 2026-10-30: met in part. All 5 bridges run on codecs under `codecEngine = ir` (CCS-176/177/178), and `proxy_golden` passes 18/18, including the `golden_<bridge>_ir` run for every bridge. The intentional IR differences are listed in CCS-176/177/178.
  - Unmet: the default engine is still `legacy`. The 10 legacy files (`providers/transform{,_responses,_codex_anthropic,_codex_chat,_gemini}.rs` and `providers/streaming{...}.rs`, 18,397 lines today) are still present, because CCS-178 gates their deletion on one release soak.
  - Net reduction: before CCS-174 (9b32838) the legacy files were 20,307 lines. `proxy/codec/` is now 17,459 lines, about 5.2k of them in `tests.rs`-style files. After the deletion, this pair alone nets out at about -2.8k lines, not -8k. CCS-178's estimate of about 9k counted only that ticket's own codec additions.
- [ ] cached_tokens/cache_creation_input_tokens appear only in codec and usage modules; the parity checker reports 0 mismatches over the corpus
  - Audit: the parity half is met. `cargo test --test proxy_golden` passes 18/18 today, including `usage_parity_golden_corpus` (108 bodies per engine, legacy and IR, CCS-186) and the 5 `codec_parity_<bridge>` tests.
  - The grep half is unmet. Outside `codec/` and `usage/`, the token names still appear in 5 legacy files, which go only when those files are deleted:
    - `streaming.rs`: 6
    - `transform.rs`: 6
    - `transform_codex_anthropic.rs`: 3
    - `transform_codex_chat.rs`: 5
    - `transform_responses.rs`: 3
  - After the deletion, three files still match. The criterion has to allow them, or they have to move under `usage/`:
    - `proxy/stream/taps.rs`: 4, the usage tap
    - `services/session_usage.rs`: 9, reads Claude Code session logs
    - `proxy/providers/testkit.rs`: 5, test fixture data

    The first two are usage accounting, not protocol conversion.

- [ ] commands/misc.rs is gone; no command file exceeds 600 lines; lib.rs run() is 300 lines or fewer; no \*\_test_hook remains; 100% of commands return IpcError
  - Audit: met except for one file.
    - `commands/misc.rs` is deleted (CCS-179).
    - `lib.rs` is 594 lines, and `run()` is 37 of them (lines 148-184, CCS-182).
    - The only `_test_hook` match is a doc comment in `ctx.rs:6` saying the hooks are gone.
    - `node scripts/count-ipc-error-commands.mjs` reports 302/302 (100.0%). `check-command-bodies.mjs` reports all 302 commands at 40 lines or fewer.
  - Unmet: `commands/settings.rs` is **1021** lines. Its code above the `#[cfg(test)]` module is only 439 lines, and the inline test module adds the other 582. The next largest file is `import_export.rs` at 418. The fix is to move that test module into its own file (for example `commands/settings/tests.rs`) or to split the file.
- [x] Per-domain AppManifest permissions; privileged commands are not in the default capability
  - Met (CCS-180):
    - `src-tauri/permissions/` has 16 domain permission sets plus `privileged-{env,terminal,file-write}.toml`.
    - `capabilities/default.json` grants only the 16 standard domains.
    - The 3 privileged sets are granted only by `capabilities/privileged.json` (`local: true`, main window).
- [ ] proxy/ and database/ line coverage at least 75%, ratcheted
  - Audit: unmet, not measured.
    - The only recorded numbers are the P0 baseline from CCS-019: proxy/ 78.60% and database/ 78.84% of lines. Nobody has measured coverage since P3-P5 added code.
    - Not re-run here: `cargo llvm-cov` builds a second, instrumented target dir and runs the whole suite. The disk has about 5 GB free and the machine is shared.
    - Nothing is ratcheted yet. The `coverage-rust` CI job is report-only (`continue-on-error`), and `scripts/coverage-summary.mjs` has no thresholds.
- [ ] Proxy fix commits per release are below the P0 baseline window
  - Audit: cannot be measured yet. The repo has no release tags, v4.0 has not shipped, and no P0 baseline count of proxy fix commits is recorded. The plan (`next-phase-plan.md`) tracks this KPI in release notes.

## Child tickets

p5-ir-codec-core, p5-session-side-store, p5-codec-openai-chat, p5-codec-gemini, p5-codec-responses, p5-misc-split-terminal, p5-command-domains-permissions, p5-thin-commands, p5-app-shell-bootstrap, p5-tray-split, p5-session-resume-cross-platform, p5-midstream-failover, p5-usage-parity-ci, p5-gemini-cli-anthropic, p5-tool-lifecycle-streaming

## Epic closure audit

Audited 2026-10-30 on `claude/funny-carson-9c7j3e` (c0c770a, after the lane-2 P5 command-layer merge). The audit changed no code. **Status: Partial**: 1 of 6 exit criteria is met in full.

| Ticket  | Key                              | Status  |
| ------- | -------------------------------- | ------- |
| CCS-174 | p5-ir-codec-core                 | Done    |
| CCS-175 | p5-session-side-store            | Done    |
| CCS-176 | p5-codec-openai-chat             | Done    |
| CCS-177 | p5-codec-gemini                  | Done    |
| CCS-178 | p5-codec-responses               | Partial |
| CCS-179 | p5-misc-split-terminal           | Partial |
| CCS-180 | p5-command-domains-permissions   | Done    |
| CCS-181 | p5-thin-commands                 | Done    |
| CCS-182 | p5-app-shell-bootstrap           | Done    |
| CCS-183 | p5-tray-split                    | Done    |
| CCS-184 | p5-session-resume-cross-platform | Partial |
| CCS-185 | p5-midstream-failover            | Done    |
| CCS-186 | p5-usage-parity-ci               | Done    |
| CCS-187 | p5-gemini-cli-anthropic          | Done    |
| CCS-188 | p5-tool-lifecycle-streaming      | Done    |

**Done.**

- **Codecs:** all five bridges have an IR codec path behind `codecEngine = ir`. Parity with the legacy engine holds over the whole golden corpus: `proxy_golden` passes 18/18, the 5 `codec_parity_*` tests pass, and `usage_parity_golden_corpus` checks 108 bodies per engine.
- **Session side store, mid-stream failover and the Gemini CLI ← Anthropic bridge** are done (CCS-175, CCS-185, CCS-187).
- **Command layer:**
  - `misc.rs` is split into `services/cli_tools/` and `platform/terminal/` (CCS-179).
  - Commands are registered per domain, with ACL permissions and a separate privileged capability (CCS-180).
  - Commands are thin adapters over `Ctx`/`EventSink`. 302/302 return `IpcError`, all are at 40 lines or fewer, and no `*_test_hook` remains (CCS-181).
  - `StartupStep` bootstrap: `run()` is 37 lines (CCS-182).
  - The tray module is split (CCS-183).
  - Session resume is wired on all 3 OSes (CCS-184).
  - Tool lifecycle output streams to the UI (CCS-188).

**Remaining.**

1. **Delete the legacy engine** (criteria 1 and 2, CCS-178). This is gated on one release that ships with `codecEngine = ir` and has no regressions; the default is still `legacy`.
   - After that release: delete the 10 `providers/transform*.rs` / `streaming*.rs` files and the `CodecEngine` flag, then repoint the re-export callers at `codec::*`.
   - The −8k net-line target looks out of reach as measured. Legacy was 20,307 lines before CCS-174 and `proxy/codec/` is now 17,459, so the deletion nets about −2.8k. Either the target is revised, or the codec tests that duplicate legacy tests get trimmed when the parity tests go.
   - The grep criterion also needs a decision about `stream/taps.rs`, `services/session_usage.rs` and `providers/testkit.rs`, which are usage accounting or test data, not conversion code.
2. **Split `commands/settings.rs`** (criterion 3). It is 1021 lines, 582 of them inline tests. Moving the tests to their own file is a small code change and finishes the criterion.
3. **Coverage gate** (criterion 5):
   - Run `cargo llvm-cov` in CI, or on a machine with free disk, to get current proxy/ and database/ numbers.
   - Then turn the report-only `coverage-rust` job into a ratcheting threshold: add a `--min` option to `coverage-summary.mjs` and drop `continue-on-error`.
4. **Release KPI** (criterion 6): can only be measured after v4.0 ships. It needs a P0 baseline count of proxy fix commits, plus release tags to count against.
5. **Partial child tickets that need CI or manual checks.** These do not block a specific exit criterion, but they keep their tickets open:
   - CCS-179: the macOS terminal tests need a macOS CI run. Windows already ran under Wine.
   - CCS-184: resume needs a manual check on real Windows and Linux desktops.

### Remaining after triage

- Blocked: Make codecEngine=ir the default in a release and delete the 10 legacy transform/streaming files and the flag. Time-gated: one release with ir and no regressions (CCS-246 prepares the flip).
- Blocked: Net line reduction ≥ 8k. Needs a human decision: the deletion nets about -2.8k, so the target must be revised or duplicated codec tests trimmed.
- Blocked: Grep criterion for taps.rs/session_usage.rs/testkit.rs. Needs a human decision to allow them, unless CCS-244 moves them.
- Blocked: Run the ratcheted coverage-rust gate. Needs a real GitHub Actions run (CI triggers only on main/PRs; the repo has no Actions history).
- Blocked: Proxy fix commits per release below the P0 baseline. Needs v4.0 shipped with release tags.
- Blocked: CCS-179 macOS terminal tests and CCS-184 manual resume on real Windows/Linux desktops. Need a Mac or macOS CI, and interactive desktop use.
- Follow-up [CCS-222](../P0-safety-net/CCS-222-p0-coverage-remeasure-baseline.md): Re-measure the coverage baseline on a clean tree and record proxy/ and database/ numbers.
- Follow-up [CCS-241](../P5-protocol-ir-commands/CCS-241-p5-commands-settings-test-split.md): Move the commands/settings.rs test module into its own file.
- Follow-up [CCS-242](../P5-protocol-ir-commands/CCS-242-p5-coverage-min-threshold.md): Add a ratcheting --min threshold to coverage-summary.mjs and gate coverage-rust on it.
- Follow-up [CCS-243](../P5-protocol-ir-commands/CCS-243-p5-usage-token-name-check.md): Add a check script that keeps cache-token field names inside codec/ and usage/.
- Follow-up [CCS-244](../P5-protocol-ir-commands/CCS-244-p5-usage-token-names-out-of-taps.md): Move cache-token field parsing out of stream taps, session_usage and the provider testkit.
- Follow-up [CCS-246](../P5-protocol-ir-commands/CCS-246-p5-codec-engine-ir-default.md): Prepare the default flip to codecEngine = ir (merge needs a maintainer release decision).
- Follow-up [CCS-247](../P5-protocol-ir-commands/CCS-247-p5-terminal-applescript-builders-neutral.md): Move the macOS AppleScript builders into an OS-neutral module so their tests run on Linux.
- Follow-up [CCS-248](../P5-protocol-ir-commands/CCS-248-p5-e2e-session-resume-linux.md): Automate the Linux session-resume check under xvfb with a fake claude.
