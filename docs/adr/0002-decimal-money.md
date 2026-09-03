# ADR-0002 — Money is `rust_decimal`, never a float

**Status:** Accepted · **Date:** 2026-09-03

## Context

`0.1 + 0.2 != 0.3` in binary floating point. In a ledger, that difference is a
verification that doesn't balance, and a merchant's books that don't tie out.

## Decision

`Money` wraps `rust_decimal::Decimal`, scale 2 (öre). Floats appear nowhere in
the codebase — not in tests, not in fixtures, not in the event schema, where
amounts are JSON **strings** (`"1000.00"`) rather than numbers, because a JSON
number is a float in most parsers including JavaScript's.

That last point matters more than it looks: the primary integration target is a
Node host, and `JSON.parse` will silently turn `1000.10` into a float.

Rounding: half-away-from-zero, applied **per line, then summed**. Never sum then
round — the results differ, and only the per-line result matches what the host's
checkout charged the customer.

`Money` carries a currency tag even though v0.1 is SEK-only, so adding
currencies later is a change in one place rather than everywhere.

## Consequences

**Good**

- Exact arithmetic. Verifications balance because the numbers are right, not
  because tolerance was added
- Currency mismatches are a compile error, not a runtime surprise
- The string-in-JSON choice removes an entire class of silent corruption at the
  JS boundary

**Bad**

- Slower than `f64`. Irrelevant here
- `rust_decimal` is a dependency in the core, permitted explicitly under
  [03 §8](../03-architecture.md#8-dependency-policy)
- Event JSON is slightly less natural to hand-write. Worth it

## Alternatives rejected

- **`i64` minor units (öre)** — exact and dependency-free, but VAT at 25% of an
  odd öre amount needs intermediate precision, and every call site has to
  remember the scale. A real option; `rust_decimal` chosen for ergonomics.
- **`f64` with an epsilon tolerance** — the thing this ADR exists to prevent.
