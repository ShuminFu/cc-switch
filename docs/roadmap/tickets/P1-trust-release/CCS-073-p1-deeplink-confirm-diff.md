# CCS-073: [deeplink] Show a full import preview with a risk diff in the confirm dialog

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P1`, `p2-medium`, `area:deeplink`

> **Ticket key:** `p1-deeplink-confirm-diff` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p1-deeplink-env-allowlist`, `p1-deeplink-script-disabled`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
This combines backend idea 2, frontend idea 4 and the plan backlog's "import preview with trust indicator". Users approve deeplinks without seeing the env keys, MCP commands or usage-script code that will be written, or how they differ from an existing provider with the same name.

## Proposed change
- Reuse merge_deeplink_config to return a structured preview: env keys (flagged by the allow-list), MCP server commands, the usage-script source and target host, and the live file(s) that will change.
- Render a diff against the existing provider with the same name, and add a "replaced by a newer link" banner.
- Mark risky items in red and require an extra checkbox before Import is enabled.

## Acceptance criteria / verification
- [ ] RTL test: a link with an MCP command and a script renders both in full; Import stays disabled until the risk box is checked
- [ ] `pnpm test:unit`, `pnpm typecheck`, `cargo test --manifest-path src-tauri/Cargo.toml deeplink`

## Dependencies
p1-deeplink-env-allowlist, p1-deeplink-script-disabled
