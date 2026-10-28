# CCS-048: [docs] Move the session-manager PRD into docs/ and leave the Pages-served deplink.html alone

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:docs`

> **Ticket key:** `p0-docs-root-clutter` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-root-clutter, partially real. session-manager.md (268-line PRD) sits at the repo root and nothing references it. deplink.html is served by GitHub Pages and linked from 6 user-manual pages, so it must stay.

## Evidence
- /session-manager.md; docs/user-manual/{en,ja,zh}/2-providers/2.1-add.md:352 and 5-faq/5.3-deeplink.md:16 link deplink.html

## Proposed change
Move session-manager.md to docs/roadmap/. Add a comment at the top of deplink.html saying it is served by Pages.

## Acceptance criteria / verification
- [x] `grep -rn session-manager.md . --exclude-dir=node_modules` finds no broken reference
- [ ] https://farion1231.github.io/cc-switch/deplink.html still loads

## Dependencies
None.

## Implementation notes
- **Summary:** `git mv session-manager.md docs/roadmap/session-manager.md` (content unchanged, rename detected at 100%; the PRD has no relative links, so nothing inside it breaks). `deplink.html` stays at the repo root; an HTML comment directly after the `<!DOCTYPE html>` line now says it is served by GitHub Pages at https://farion1231.github.io/cc-switch/deplink.html, names the user-manual pages that link to it, and says to keep it at the root. The comment comes after the doctype, so the page still renders in standards mode.
- **Files:** `session-manager.md` -> `docs/roadmap/session-manager.md`, `deplink.html`, this ticket file.
- **Commits:** `2885f72` (move + comment), plus the follow-up commit that records these notes.
- **Verification:**
  - `grep -rn session-manager.md . --exclude-dir=node_modules --exclude-dir=.git --exclude-dir=target`: the only hits are inside this ticket file (prose). No links to the old path anywhere, so nothing is broken.
  - `python3` `html.parser` parses `deplink.html` cleanly. `git show --stat 2885f72` shows the move as a pure rename (0 lines changed) and +5 comment lines in `deplink.html`.
  - No code changed, so typecheck, unit tests and cargo were not needed. `pnpm format:check` only covers `{src,tests}`, so it does not check these files.
- **Remaining (not checkable here):** confirming https://farion1231.github.io/cc-switch/deplink.html still loads needs a Pages deploy from the default branch after merge. The file path and name did not change, and the only edit is an HTML comment, so the served URL is unaffected.
