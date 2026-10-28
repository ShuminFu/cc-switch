# CCS-045: [docs] Add an [Unreleased] CHANGELOG section that lists Grok Build and other post-3.17 changes

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:docs`

> **Ticket key:** `p0-docs-changelog-unreleased` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-readme-unreleased-feature. The README advertises "Eight Tools" including Grok Build (#5453), but v3.17.0 does not ship it and the CHANGELOG has no [Unreleased] section.

## Evidence
- README.md:5,202,222,257; CHANGELOG.md:8; `git log --oneline c154d30..HEAD`

## Proposed change
Add `## [Unreleased]` listing #5453, #4355, #5356, #5508, #5310, #4706 and #5138, or mark Grok Build as "available on main" in the README.

## Acceptance criteria / verification
- [x] `grep -n '^## \[Unreleased\]' CHANGELOG.md` and `grep -in 'grok build' CHANGELOG.md` both match

## Dependencies
None.

## Implementation notes
- **Summary:** Added a `## [Unreleased]` section above `## [3.17.0]` in `CHANGELOG.md`. It says that Grok Build is only on `main` (not in v3.17.0) and lists every post-3.17.0 merge from `git log c154d30..HEAD`: Added: Grok Build (#5453). Fixed: tray first-run locale (#4355), deleted default skill repos (#5356), Codex reasoning forwarding (#5508), streamed tool-call identity/order (#5310), function-parameter normalization (#4706), the missing `proxyReasonAnthropicMessages` i18n key. CI: Windows/macOS backend matrix (#5138). The Grok Build entry also mentions the v13→v14→v15 schema migrations. The README is unchanged; the changelog now explains the "Eight Tools" claim.
- **Files:** `CHANGELOG.md`, this ticket file.
- **Commits:** b0594c3 (changelog); the ticket-file commit follows it.
- **Verification:**
  - `grep -n '^## \[Unreleased\]' CHANGELOG.md` → `8:## [Unreleased]`
  - `grep -in 'grok build' CHANGELOG.md` → matches lines 10 and 14
  - `npx prettier --parser markdown` makes no changes to the new section. The whole `CHANGELOG.md` already failed `prettier --check` before this change, and `pnpm format:check` does not cover Markdown.
  - This is a docs-only change, so no code tests apply.
