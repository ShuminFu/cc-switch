# CCS-006: [epic] P5 (v4.0): Canonical protocol IR and codecs, plus command-layer cleanup

**Status:** Partial · **Kind:** epic · **Labels:** `epic`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `epic-p5` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** XL · **Depends on:** `epic-p4`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal

Fix the root cause of bridge drift by expressing every bridge as decode(upstream) -> IR -> encode(client), one bridge per release candidate. Finish extracting the command layer and the app shell, including per-domain capability permissions (moved here by the critique, after collect_commands!).

## Exit criteria

- [ ] All 5 bridges run on codecs; the old providers/transform*.rs and streaming*.rs files are deleted; net reduction of at least 8k lines; fixtures pass or the diffs are documented
  - Re-audit 2026-10-31 (673be73): met in part.
  - Met: all 5 bridges run on codecs, and `codecEngine = ir` is now the default (CCS-246: `CodecEngine::Ir` is `#[default]` and `AppSettings::default()` sets it; `legacy` is an explicit fallback). `cargo test --test proxy_golden` passes 18/18 today, including `golden_<bridge>_ir` for every bridge. The intentional IR differences are listed in CCS-176/177/178, plus the forwarder wire golden change in CCS-246.
  - Met: non-legacy code no longer imports through the legacy modules (CCS-245). `proxy::providers::tests::legacy_transform_modules_are_only_referenced_by_allowed_files` guards this.
  - Unmet: the 10 legacy files (`providers/transform{,_responses,_codex_anthropic,_codex_chat,_gemini}.rs` and `providers/streaming{,_responses,_codex_anthropic,_codex_chat,_gemini}.rs`, 18,396 lines today) and the `CodecEngine` flag are still present. CCS-178 gates their deletion on one release that ships with `ir` as the default and has no regressions.
  - Unmet: net reduction. Before CCS-174 (9b32838) the legacy files were 20,307 lines. `proxy/codec/` is now 17,482 lines, about 5.2k of them in test files. After the deletion this pair nets about -2.8k lines, not -8k.
- [ ] cached_tokens/cache_creation_input_tokens appear only in codec and usage modules; the parity checker reports 0 mismatches over the corpus
  - Re-audit 2026-10-31: the parity half is met. `proxy_golden` passes 18/18 today, including `usage_parity_golden_corpus` (108 bodies per engine, CCS-186) and the 5 `codec_parity_<bridge>` tests.
  - The grep half is met except for the legacy files. CCS-244 moved the field reads out of `proxy/stream/taps.rs`, `services/session_usage.rs` and `proxy/providers/testkit.rs` into `proxy/usage/anthropic_fields.rs` and `proxy/usage/test_data.rs`. Outside `codec/` and `usage/`, only 5 files still match, all legacy: `streaming.rs`, `transform.rs`, `transform_codex_anthropic.rs`, `transform_codex_chat.rs` and `transform_responses.rs`.
  - CCS-243 enforces this in CI. `node scripts/check-usage-token-names.mjs` passes today with "5 allowlisted legacy files", and it fails if an allowlisted file stops matching, so the allowlist has to empty when the legacy files are deleted.
  - The criterion is fully met when CCS-178 deletes the legacy files.
- [x] commands/misc.rs is gone; no command file exceeds 600 lines; lib.rs run() is 300 lines or fewer; no \*\_test_hook remains; 100% of commands return IpcError
  - Re-audit 2026-10-31: met.
    - `commands/misc.rs` is deleted (CCS-179).
    - The largest command files are `settings_tests.rs` (533) and `settings.rs` (508), after CCS-241 moved the inline test module out. Every file under `commands/` is at or under 600 lines.
    - `lib.rs` is 594 lines, and `run()` is 37 of them (lines 148-184, CCS-182).
    - The only `_test_hook` match is a doc comment in `ctx.rs:6` saying the hooks are gone.
    - `node scripts/count-ipc-error-commands.mjs` reports 302/302 (100.0%). `check-command-bodies.mjs` reports all 302 commands at 40 lines or fewer, with no direct DB access.
- [x] Per-domain AppManifest permissions; privileged commands are not in the default capability
  - Met (CCS-180):
    - `src-tauri/permissions/` has 16 domain permission sets plus `privileged-{env,terminal,file-write}.toml`.
    - `capabilities/default.json` grants only the 16 standard domains.
    - The 3 privileged sets are granted only by `capabilities/privileged.json` (`local: true`, main window).
- [x] proxy/ and database/ line coverage at least 75%, ratcheted
  - Re-audit 2026-10-31: met in code. It has not yet run on GitHub Actions.
    - CCS-222 measured a clean tree at a64ab58 (`cargo llvm-cov`, 3114 tests passed, 0 failed): proxy/ 92.27% of lines (49568/53723), database/ 85.70% of lines (7709/8995). Both are above the 75% target. The numbers are in `docs/dev/perf-baseline.json`.
    - CCS-242 made the gate: `scripts/coverage-summary.mjs --min <dir>=<pct>` exits 1 when a directory is below its floor. The `coverage-rust` job in `ci.yml` no longer has `continue-on-error` and passes `--min src-tauri/src/proxy=75 --min src-tauri/src/database=75`. Only `coverage-frontend` is still report-only.
    - The floors sit at the 75% target, not at the measured baseline. Raising them is an ordinary ratchet step.
- [ ] Proxy fix commits per release are below the P0 baseline window
  - Re-audit 2026-10-31: cannot be measured yet. The repo has no release tags (`git tag` is empty), and v4.0 has not shipped. No P0 baseline count of proxy fix commits is recorded. The plan (`next-phase-plan.md`) tracks this KPI in release notes.

## Child tickets

p5-ir-codec-core, p5-session-side-store, p5-codec-openai-chat, p5-codec-gemini, p5-codec-responses, p5-misc-split-terminal, p5-command-domains-permissions, p5-thin-commands, p5-app-shell-bootstrap, p5-tray-split, p5-session-resume-cross-platform, p5-midstream-failover, p5-usage-parity-ci, p5-gemini-cli-anthropic, p5-tool-lifecycle-streaming

## Epic closure audit

Re-audited 2026-10-31 on `claude/funny-carson-9c7j3e` (673be73), after the follow-up wave and the lane-2 follow-up merge. This audit changed no code. It re-ran `cargo test --test proxy_golden` (18 passed) and the read-only checks quoted above. **Status: Partial**: 3 of 6 exit criteria are met. The other 3 are blocked only by the release soak, the release KPI and one human decision about the line target.

### Child and follow-up tickets

| Ticket  | Key                                      | Status  |
| ------- | ---------------------------------------- | ------- |
| CCS-174 | p5-ir-codec-core                         | Done    |
| CCS-175 | p5-session-side-store                    | Done    |
| CCS-176 | p5-codec-openai-chat                     | Done    |
| CCS-177 | p5-codec-gemini                          | Done    |
| CCS-178 | p5-codec-responses                       | Partial |
| CCS-179 | p5-misc-split-terminal                   | Partial |
| CCS-180 | p5-command-domains-permissions           | Done    |
| CCS-181 | p5-thin-commands                         | Done    |
| CCS-182 | p5-app-shell-bootstrap                   | Done    |
| CCS-183 | p5-tray-split                            | Done    |
| CCS-184 | p5-session-resume-cross-platform         | Partial |
| CCS-185 | p5-midstream-failover                    | Done    |
| CCS-186 | p5-usage-parity-ci                       | Done    |
| CCS-187 | p5-gemini-cli-anthropic                  | Done    |
| CCS-188 | p5-tool-lifecycle-streaming              | Done    |
| CCS-222 | p0-coverage-remeasure-baseline           | Done    |
| CCS-241 | p5-commands-settings-test-split          | Done    |
| CCS-242 | p5-coverage-min-threshold                | Done    |
| CCS-243 | p5-usage-token-name-check                | Done    |
| CCS-244 | p5-usage-token-names-out-of-taps         | Done    |
| CCS-245 | p5-proxy-repoint-legacy-reexports        | Done    |
| CCS-246 | p5-codec-engine-ir-default               | Done    |
| CCS-247 | p5-terminal-applescript-builders-neutral | Done    |
| CCS-248 | p5-e2e-session-resume-linux              | Done    |
| CCS-249 | p5-terminal-resume-bat-wine              | Done    |

### Done

- **Codecs:** all five bridges run on IR codecs, and `ir` is now the default engine (CCS-246). Parity with the legacy engine holds over the golden corpus: `proxy_golden` passes 18/18, including the 5 `codec_parity_*` tests and `usage_parity_golden_corpus`. Non-legacy code no longer imports through the legacy modules, and a test guards this (CCS-245).
- **Usage field names:** they are now read only in `codec/` and `usage/`, apart from the 5 legacy files (CCS-244). `check-usage-token-names.mjs` enforces this in CI (CCS-243).
- **Command layer:** criterion 3 is fully met. `misc.rs` is gone, `settings.rs` is split (CCS-241), every command file is at or under 600 lines, `run()` is 37 lines, there are no test hooks, and 302/302 commands return `IpcError`.
- **Permissions:** per-domain ACL with a separate privileged capability (CCS-180).
- **Coverage:** proxy/ 92.27% and database/ 85.70% (CCS-222). `coverage-rust` is now a blocking gate with 75% floors (CCS-242).
- **Terminal and resume:** the AppleScript builders and their 8 tests now run on Linux (CCS-247). The Linux resume path runs end to end in xterm under Xvfb (CCS-248). The Windows resume batch runs under Wine `cmd` (CCS-249).
- Session side store, mid-stream failover, the Gemini CLI ← Anthropic bridge, the tray split, `StartupStep` bootstrap and tool lifecycle streaming are done (CCS-175, 185, 187, 183, 182, 188).

### Remaining

1. **Release soak, then delete the legacy engine** (criteria 1 and 2, CCS-178). This is a deliberate time gate in the plan: one release that ships with `codecEngine = ir` as the default and has no regressions. The flip is merged on this branch (CCS-246), but a maintainer has to schedule that release. After it ships: delete the 10 `providers/transform*.rs` / `streaming*.rs` files (18,396 lines), the `CodecEngine` flag, the legacy arms in `codec/bridge.rs` and the test-only legacy imports in `stream/codec.rs`, then empty the `check-usage-token-names.mjs` allowlist. That deletion finishes criterion 2.
2. **Net line reduction ≥ 8k** (criterion 1). This needs a human decision. As measured, the deletion nets about -2.8k (legacy was 20,307 lines before CCS-174; `proxy/codec/` is now 17,482). Either the target is revised, or the codec tests that duplicate legacy tests are trimmed when the parity tests go.
3. **First real `coverage-rust` run** (criterion 5, already ticked). The blocking gate has only been checked locally against the CCS-222 lcov. It needs a GitHub Actions run on a PR or on `main`.
4. **Release KPI** (criterion 6). This can only be measured after v4.0 ships with signed release tags, and a P0 baseline count of proxy fix commits has to be recorded to compare against.
5. **Partial child tickets that need other machines.** They do not block an exit criterion:
   - CCS-179: the `osascript`/`open` launchers in `macos.rs` need a native macOS run (the macos-latest CI leg or a Mac).
   - CCS-184: resume needs a manual GUI check on a real Windows desktop (cmd /K, PowerShell, Windows Terminal, the real npm `claude.cmd` shim) and on real gnome-terminal/konsole.
   - CCS-246 and CCS-244: their PRs need the CCS-034 fixture label (`fixtures` / `no-fixture-needed`) set on GitHub.
