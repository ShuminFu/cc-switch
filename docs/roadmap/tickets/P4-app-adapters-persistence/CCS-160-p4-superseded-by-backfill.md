# CCS-160: [db] Replace the correlated EXISTS with a superseded_by column and a chunked, resumable backfill

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:database`

> **Ticket key:** `p4-superseded-by-backfill` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-usage-repo`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Dashboard queries run a correlated EXISTS. Per the critique, a startup backfill on a large proxy_request_logs table would block startup for a long time.

## Evidence
- usage_stats.rs:297-336

## Proposed change
- Add the superseded_by column.
- Run the backfill in the background in 10k-row chunks with a persisted cursor and progress events. It resumes after a crash, and queries fall back to EXISTS until it completes.

## Acceptance criteria / verification
- [ ] On a 1M-row synthetic DB: startup is not blocked; the dashboard summary p95 is 50 ms or less after the backfill; results equal the EXISTS logic (parity test)
- [ ] A kill mid-backfill resumes from the cursor
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml superseded`

## Dependencies
p4-usage-repo
