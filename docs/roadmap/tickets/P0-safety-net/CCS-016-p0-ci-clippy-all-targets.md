# CCS-016: [ci] Run clippy with --all-targets

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci`

> **Ticket key:** `p0-ci-clippy-all-targets` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-clippy-all-targets: clippy lints only the lib/bin targets, so about 133 test modules and src-tauri/tests/* are never linted.

## Evidence
- .github/workflows/ci.yml:102 `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`

## Proposed change
Fix the existing test-code warnings, then change the step to `--all-targets`.

## Acceptance criteria / verification
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` passes locally (with a dist/ dir)
- [x] A deliberate lint added in tests/ fails CI (then remove it)

## Dependencies
None.

## Implementation notes

**Summary:** The CI Clippy step now runs with `--all-targets`, so the integration tests in `src-tauri/tests/*` and the inline `#[cfg(test)]` modules are linted with `-D warnings`. The test code was already warning-free at this point, so no Rust source changes were needed. The pre-submit command in `CONTRIBUTING.md` (EN and ZH) now matches CI (`cargo clippy --all-targets -- -D warnings`).

**Files:**
- `.github/workflows/ci.yml`: the Clippy step now passes `--all-targets`
- `CONTRIBUTING.md`: the "run all checks before submitting" command matches CI

**Commits:** `dfddc49` (ci change), plus this ticket update.

**Verification:**
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` (with `dist/` present): exit 0, no warnings.
- Deliberate lint probe: added a temporary `src-tauri/tests/zz_ccs016_lint_probe.rs` containing `vec!` misuse, `len() == 0` and `|| true`.
  - Old command (no `--all-targets`): exit 0, probe not linted.
  - New command (`--all-targets`): exit 101, three errors, including `useless_vec`, `len_zero` and `overly_complex_bool_expr`.
  - The probe file was then deleted and is not committed. The same failure will happen on the CI runners, since the step runs this exact command.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.
- `python3 -c "import yaml,sys;yaml.safe_load(open(sys.argv[1]))" .github/workflows/ci.yml`: parses. `actionlint` is not installed in this environment.
- Not verified here: the Windows and macOS matrix legs. Their cfg-gated test code is only linted on those runners, so the first CI run should confirm them.
