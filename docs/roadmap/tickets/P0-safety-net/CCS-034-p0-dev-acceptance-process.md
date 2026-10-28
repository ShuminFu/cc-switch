# CCS-034: [process] Add the ACCEPTANCE playbook, the fixture-required PR rule and the fix-commit KPI

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:process`

> **Ticket key:** `p0-dev-acceptance-process` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p0-golden-testkit-chat-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The plan's process rules need to be written down and enforced. They are: every proxy bug fix adds a fixture first, structural PRs keep the old path callable, and fix commits per release are tracked as a KPI. Contributors also lack a how-to for adding an app type (CI idea 10).

## Proposed change
- Add docs/dev/ACCEPTANCE.md covering "add an app" and "fix a proxy bug". Add a CLAUDE.md/CONTRIBUTING pointer.
- Add .github/pull_request_template.md with a fixture checkbox.
- Add a labeler plus a check: a change under src-tauri/src/proxy/** needs a tests/fixtures/proxy change or the `no-fixture-needed` label.
- Add a scripts/kpi-fix-commits.sh baseline over the last 3 releases.

## Acceptance criteria / verification
- [ ] A test PR touching proxy/ without a fixture or label fails the check
- [ ] The baseline KPI is recorded in docs/dev

## Dependencies
p0-golden-testkit-chat-gemini
