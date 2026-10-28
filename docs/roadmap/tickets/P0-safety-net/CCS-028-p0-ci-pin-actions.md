# CCS-028: [ci] Pin every GitHub Action to a commit SHA

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P0`, `p1-high`, `area:ci`

> **Ticket key:** `p0-ci-pin-actions` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-unpinned-actions. The release job (environment: release) holds the Tauri updater key, the Apple cert and the notarization credentials, and runs dtolnay/rust-toolchain@stable (a branch), pnpm/action-setup@v6, actions/cache@v5 and others by mutable tag. claude.yml exposes CLAUDE_CODE_OAUTH_TOKEN through anthropics/claude-code-action@v1.

## Evidence
- release.yml:33,36,41,87,110,563,608; claude.yml:50; ci.yml

## Proposed change
Pin each `uses:` to a 40-character SHA with a version comment. Change the dependabot github-actions schedule to weekly.

## Acceptance criteria / verification
- [ ] `grep -nE 'uses: [^@]+@' .github/workflows/*.yml | grep -vE '@[0-9a-f]{40}'` prints nothing
- [ ] zizmor/actionlint report no unpinned-uses

## Dependencies
None.
