# ADR-0005 — The event schema versions independently of the crate

**Status:** Accepted · **Date:** 2026-09-03

## Context

Hosts persist the event log. Those events are räkenskapsinformation, which under
Bokföringslagen must remain available for seven years after the end of the
calendar year in which the financial year closed.

So a v1 event written in 2026 must still parse in 2033 — while the library
itself will go through many releases in that time.

Tying schema compatibility to crate SemVer conflates two lifecycles with very
different clocks.

## Decision

The event and config schemas carry their own integer `schema_version`,
independent of the crate version.

- A **patch or minor** crate release must never change the schema
- A **schema bump** does not force a major crate bump if the Rust API is unchanged
- The deserialiser supports version **N and N−1**
- Dropping N−2 is a major crate release with a written migration note
- Conventional Commits scope `schema:` marks any change touching either schema,
  to force a deliberate pause

Pre-1.0, the Rust API may churn freely. **The event schema may not.**

## Consequences

**Good**

- Retention obligation is met by design rather than by remembering
- The library can be refactored aggressively without breaking stored data
- `schema:` in a commit message is a visible signal in review

**Bad**

- Two version numbers to explain in the docs
- N and N−1 support means migration code lives in the codebase indefinitely
- Adding a field to `SaleLine` is a schema decision, not a casual refactor. This
  friction is intentional

## Alternatives rejected

- **Schema follows crate SemVer** — simpler, but ties a seven-year data format to
  a fast-moving library version.
- **Self-describing events (embedded schema)** — robust, verbose, and overkill
  for four event types.
