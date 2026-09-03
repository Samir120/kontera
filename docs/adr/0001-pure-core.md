# ADR-0001 — The core is a pure function

**Status:** Accepted · **Date:** 2026-09-03

## Context

The obvious design for a bookkeeping subsystem is a service: a database of
verifications, an API to post to it, background jobs to reconcile. That is how
every product in this space works.

But the host platform already stores orders. A second store means two sources of
truth, sync logic, drift, and a reconciliation problem *about the reconciliation
system*.

## Decision

`kontera-core` exposes one function:

```rust
pub fn post(events: &[Event], config: &Config) -> Result<Ledger, PostingError>;
```

No I/O, no async, no database, no clock, no randomness, no `unsafe`. Enforced in
CI by `scripts/check-purity.sh`.

The host stores events. The library computes. Period open/closed state is passed
in via `Config` rather than remembered.

## Consequences

**Good**

- Property testing over generated event logs is possible at all — this is what
  makes the invariants in [02 §6](../02-domain-model.md) provable rather than
  aspirational
- No migrations, no drift, no state corruption. Wrong output means fix the code
  and re-run; there is nothing to repair
- Compiles to WASM with no runtime, so `npm install` is a viable integration
- Zero operational burden on the host — no service to deploy or monitor

**Bad**

- Full recomputation every time. Irrelevant at this scale (thousands of events,
  once a day), would matter at millions
- The host carries the storage and retention obligation. This must be stated
  explicitly in the integration guide, since it is a legal duty under BFL
- No incremental posting. Adding it later would mean caching, which would mean
  state, which would mean revisiting this ADR

## Alternatives rejected

- **Service with its own database** — the standard design. Rejected because two
  sources of truth is the problem, not the solution.
- **Embedded storage (SQLite/sled)** — smaller version of the same problem, plus
  it breaks the WASM target.
