# CCS-045: [docs] Add an [Unreleased] CHANGELOG section that lists Grok Build and other post-3.17 changes

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:docs`

> **Ticket key:** `p0-docs-changelog-unreleased` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-readme-unreleased-feature. The README advertises "Eight Tools" including Grok Build (#5453), but v3.17.0 does not ship it and the CHANGELOG has no [Unreleased] section.

## Evidence
- README.md:5,202,222,257; CHANGELOG.md:8; `git log --oneline c154d30..HEAD`

## Proposed change
Add `## [Unreleased]` listing #5453, #4355, #5356, #5508, #5310, #4706 and #5138, or mark Grok Build as "available on main" in the README.

## Acceptance criteria / verification
- [ ] `grep -n '^## \[Unreleased\]' CHANGELOG.md` and `grep -in 'grok build' CHANGELOG.md` both match

## Dependencies
None.
