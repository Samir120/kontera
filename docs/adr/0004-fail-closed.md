# ADR-0004 — Fail closed on unsupported scenarios

**Status:** Accepted · **Date:** 2026-09-03

## Context

v0.1 supports four VAT scenarios. Real merchants will send gift cards,
prepayments, construction reverse charge, multi-currency orders, and sales from
merchants below the 120,000 kr VAT registration threshold.

There are two ways to handle an input outside the supported set: post something
plausible, or refuse.

## Decision

**Refuse.** `post()` returns `Err(UnsupportedScenario { event, reason })` where
`reason` is a `ScenarioGap` enum, not a string.

The same principle applies to settlement: a payout whose components don't sum to
the reported amount returns `Err(SettlementMismatch)` carrying the `delta`, never
a "close enough" posting with a rounding plug.

## Consequences

**Good**

- A library that refuses to guess is more valuable than one that always answers.
  In accounting, a wrong number that looks right is worse than no number
- It makes the narrow v0.1 scope a *feature*: the boundary is enforced, visible,
  and documented in the output
- `ScenarioGap` being an enum means the host can tell a merchant exactly which
  orders need manual handling — an actionable error, not an opaque failure
- Hosts collecting `ScenarioGap` frequencies produce a ranked list of what to
  build next, from real data rather than guesswork

**Bad**

- A single unsupported order fails a whole batch. Hosts will want partial
  results. v0.2 should consider a `post_partial()` returning
  `(Ledger, Vec<Rejected>)` — but the strict version ships first, because the
  strict version is the one that establishes the guarantee
- More upfront work: every gap needs a named variant and a clear message

## Alternatives rejected

- **Best-effort posting with warnings** — warnings get ignored, and the wrong
  number reaches a momsdeklaration.
- **Passthrough to a suspense account** — defensible in a full accounting system
  where a human reviews suspense. This library has no human and no UI.
