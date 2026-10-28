# CCS-130: [proxy] Split forward() into request_builder, HeaderPolicy, transport and validate, leaving forwarder.rs a facade

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-request-pipeline-stages` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p3-recovery-strategy`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
forward() is about 1,100 lines, and forwarder.rs is 4,769.

## Proposed change
- pipeline/request_builder.rs (BodyStages: ModelMapping, VendorNormalize, Encode, LocalOverrides, Canonicalize).
- headers.rs: a declarative HeaderPolicy instead of the saw_* flags, at forwarder.rs:1819-1983.
- transport.rs and validate.rs (2xx semantics, stream priming).

## Acceptance criteria / verification
- [ ] forwarder.rs is 600 lines or fewer; no pipeline function exceeds 150 lines
- [ ] Golden upstream_request.expected is byte-identical
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden --test proxy_e2e`

## Dependencies
p3-recovery-strategy
