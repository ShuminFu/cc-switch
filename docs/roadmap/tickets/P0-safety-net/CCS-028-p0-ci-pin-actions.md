# CCS-028: [ci] Pin every GitHub Action to a commit SHA

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P0`, `p1-high`, `area:ci`

> **Ticket key:** `p0-ci-pin-actions` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-unpinned-actions. The release job (environment: release) holds the Tauri updater key, the Apple cert and the notarization credentials, and runs dtolnay/rust-toolchain@stable (a branch), pnpm/action-setup@v6, actions/cache@v5 and others by mutable tag. claude.yml exposes CLAUDE_CODE_OAUTH_TOKEN through anthropics/claude-code-action@v1.

## Evidence
- release.yml:33,36,41,87,110,563,608; claude.yml:50; ci.yml

## Proposed change
Pin each `uses:` to a 40-character SHA with a version comment. Change the dependabot github-actions schedule to weekly.

## Acceptance criteria / verification
- [x] `grep -nE 'uses: [^@]+@' .github/workflows/*.yml | grep -vE '@[0-9a-f]{40}'` prints nothing
- [x] zizmor/actionlint report no unpinned-uses

## Dependencies
None.

## Implementation notes
- Every remote `uses:` in `.github/workflows/*.yml` is pinned to the 40-character
  commit SHA its previous major tag pointed at, with the exact release as a
  trailing comment (e.g. `actions/checkout@d23441a… # v6.1.0`) so Dependabot can
  keep bumping them. The SHAs were resolved with `git ls-remote --tags`
  (annotated tags dereferenced via `^{}`). The local reusable-workflow call
  `uses: ./.github/workflows/ci.yml` needs no pin.
- Pins: EmbarkStudios/cargo-deny-action v2.1.1, Swatinem/rust-cache v2.9.3,
  actions/attest-build-provenance v3.0.0, actions/cache v5.1.0,
  actions/checkout v6.1.0, actions/download-artifact v8.0.2, actions/labeler
  v5.0.0, actions/setup-node v6.5.0, actions/stale v10.4.0,
  actions/upload-artifact v7.0.2, anthropics/claude-code-action v1.0.264,
  pnpm/action-setup v6.0.10, softprops/action-gh-release v3.0.3,
  taiki-e/install-action v2.88.0. (`dtolnay/rust-toolchain@stable` from the
  evidence was already removed by earlier tickets; the toolchain comes from
  `rust-toolchain.toml`.)
- `.github/dependabot.yml`: the `github-actions` schedule is now `weekly`.
- Files: `.github/workflows/{audit,ci,claude,labeler,promote-release,release,stale}.yml`,
  `.github/dependabot.yml`.
- Commit: see `git log --grep CCS-028`.

Verification:
- `grep -nE 'uses: [^@]+@' .github/workflows/*.yml | grep -vE '@[0-9a-f]{40}'`
  prints nothing.
- `zizmor --offline .github/workflows` (zizmor 1.31.1): 0 `unpinned-uses`
  lines (90 against the previous HEAD). Other pre-existing zizmor findings
  (artipacked, cache-poisoning, etc.) are out of scope for this ticket. The
  online audits (`impostor-commit`, `ref-version-mismatch`) were not run: no
  GitHub token in this environment.
- `actionlint` (latest): no findings.
- All workflow files and `dependabot.yml` parse with `yaml.safe_load`.
