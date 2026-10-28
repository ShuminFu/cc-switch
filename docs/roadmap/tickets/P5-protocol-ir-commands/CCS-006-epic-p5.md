# CCS-006: [epic] P5 (v4.0): Canonical protocol IR and codecs, plus command-layer cleanup

**Status:** Open · **Kind:** epic · **Labels:** `epic`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `epic-p5` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** XL · **Depends on:** `epic-p4`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Fix the root cause of bridge drift by expressing every bridge as decode(upstream) -> IR -> encode(client), one bridge per release candidate. Finish extracting the command layer and the app shell, including per-domain capability permissions (moved here by the critique, after collect_commands!).

## Exit criteria
- [ ] All 5 bridges run on codecs; the old providers/transform*.rs and streaming*.rs files are deleted; net reduction of at least 8k lines; fixtures pass or the diffs are documented
- [ ] cached_tokens/cache_creation_input_tokens appear only in codec and usage modules; the parity checker reports 0 mismatches over the corpus
- [ ] commands/misc.rs is gone; no command file exceeds 600 lines; lib.rs run() is 300 lines or fewer; no *_test_hook remains; 100% of commands return IpcError
- [ ] Per-domain AppManifest permissions; privileged commands are not in the default capability
- [ ] proxy/ and database/ line coverage at least 75%, ratcheted
- [ ] Proxy fix commits per release are below the P0 baseline window

## Child tickets
p5-ir-codec-core, p5-session-side-store, p5-codec-openai-chat, p5-codec-gemini, p5-codec-responses, p5-misc-split-terminal, p5-command-domains-permissions, p5-thin-commands, p5-app-shell-bootstrap, p5-tray-split, p5-session-resume-cross-platform, p5-midstream-failover, p5-usage-parity-ci, p5-gemini-cli-anthropic, p5-tool-lifecycle-streaming
