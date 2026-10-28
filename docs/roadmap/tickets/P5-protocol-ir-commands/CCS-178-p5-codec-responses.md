# CCS-178: [proxy] Migrate the Responses bridges to codecs and remove the legacy engine

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-codec-responses` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** XL · **Depends on:** `p5-codec-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Migrate responses<->anthropic, then responses->chat.
- Delete the remaining transform*.rs and streaming*.rs files and the legacy flag after one release.

## Acceptance criteria / verification
- [ ] All 5 bridges run on codecs; the old files are deleted; net reduction of at least 8k lines
- [ ] cached_tokens/cache_creation_input_tokens appear only in codec and usage modules (grep)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden --test proxy_e2e`

## Dependencies
p5-codec-gemini
