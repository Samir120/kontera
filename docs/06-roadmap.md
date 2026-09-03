# 06 — Roadmap

**Status:** draft · **Owner:** Samir · **Last reviewed:** 2026-09-03

Ten weeks at evenings-and-weekends pace to a complete v0.1. Not a prototype — a
library that does one thing end to end with a test suite that proves it.

Each milestone has an exit criterion. If it isn't met, the milestone isn't done,
and the next one doesn't start.

---

## M1 · Weeks 1–2 — Types and the balance invariant

- Workspace, CI skeleton, `#![forbid(unsafe_code)]`
- `Money` with rounding rules and its own property tests
- `AccountNumber`, `AccountKind`, BAS class validation
- `Verification`, `BalancedTransaction` with a **private constructor**
- `PostingError` skeleton
- `valid_event_log()` proptest generator — the highest-leverage work in the
  project ([05 §2.1](05-testing-strategy.md#21-the-generator-is-the-hard-part))

**Exit:** an unbalanced verification cannot be constructed, proven by a property
test over generated line sets. Money rounding matches hand-worked examples.

## M2 · Weeks 3–4 — VAT and posting rules

- The four VAT scenarios ([02 §3](02-domain-model.md#3-vat-scenarios-v01))
- `Config` parsing, account mapping, missing-mapping errors
- `rules`: `SaleCaptured` → verification, `RefundIssued` → reversal,
  `FeeCharged` → expense
- `ScenarioGap` enum and fail-closed behaviour ([ADR-0004](adr/0004-fail-closed.md))
- Resolve **Q1** (per-provider receivable accounts) and **Q2** (returns strategy)

**Exit:** the worked example from [01 §1.1](01-problem-and-scope.md) produces the
exact posting table, as a unit test. Every excluded scenario returns a typed
`ScenarioGap`, never a guess.

## M3 · Weeks 5–6 — SIE 4I and the acceptance test

- `kontera-sie`: header records, `#VER`/`#TRANS`, CP437 encoding
- Golden-file harness with `KONTERA_BLESS=1`
- The `swedish-characters` fixture
- **Import into a Fortnox or Visma trial account**

**Exit — the project's real gate:** a generated `.si` imports into a commercial
Swedish system with zero manual correction, including `å ä ö`. Recorded in
`docs/acceptance/`.

> If this fails, stop and fix it. Everything after week 6 assumes the output is
> acceptable. Discovering otherwise in week 10 wastes a month.

## M4 · Weeks 7–8 — Settlement reconciliation and CLI

- `settle`: group by payout, enforce **I2**, emit the payout verification
- `SettlementSummary` and the clearing-account-nets-to-zero assertion
- The `broken_settlement_never_succeeds` property test
- `kontera-cli`: `replay`, `validate`, `--out`
- `VatReport` construction; resolve **Q3** (momsdeklaration boxes)

**Exit:** `kontera replay fixtures/basic-settlement/events.jsonl` produces the
expected file. A tampered payout amount always errors with a useful `delta`.

## M5 · Weeks 9–10 — WASM, npm, first host

- `kontera-wasm` bindings, `wasm-pack` build
- `kontera` npm package (unscoped) with TypeScript types generated from the schema
- SwadeStack adapter (in the SwadeStack repo — [03 §5](03-architecture.md#5-the-anti-corruption-layer))
- One real month of SwadeStack data through the pipeline
- Docs pass, `CHANGELOG.md`, licence decision (**Q5**)

**Exit:** SwadeStack produces a month's SIE file via `npm install`. All eight
success criteria in [01 §5.2](01-problem-and-scope.md#52-measurable-exit-criteria-for-v01)
met.

## Ordering rationale

The sequence is deliberately **risk-first, not value-first**.

The highest-risk item is not the settlement reconciler — that is hard but fully
under your control. It is whether a commercial system accepts the output, which
is not under your control at all. So SIE and the Fortnox import come at week 6,
before the settlement work, even though settlement is the more interesting
feature.

The second-highest risk is the proptest generator. If it can't produce consistent
logs, every property test becomes decorative. That is why it lands in M1
alongside the types, not later with the tests that use it.

## Risk register

| # | Risk | Impact | Mitigation |
|---|---|---|---|
| R1 | Fortnox rejects the SIE output | Project-fatal | M3, week 6, before anything depends on it |
| R2 | CP437 encoding breaks on `å ä ö` | High | Dedicated fixture; encoder returns `Err`, never mojibake |
| R3 | Default BAS mapping is wrong | High, silent | **VERIFY** markers; accountant review before release |
| R4 | proptest generator can't produce consistent logs | High | M1; generator is itself tested |
| R5 | Scope creep — multi-currency, invoicing, Peppol | Kills the timeline | [01 §4](01-problem-and-scope.md#4-non-goals-v01). New feature needs an ADR first |
| R6 | WASM binary too large for a frontend bundle | Medium | Keep core dependency-light; measure at M5; `wasm-opt` |
| R7 | Real SwadeStack data hits an unsupported scenario | Medium | Good news, actually — that's fail-closed working. Log it as v0.2 input |
| R8 | Time runs out at week 8 | Medium | M1–M4 without M5 is still a shippable CLI and a real portfolio artifact |

R8 is worth internalising. The CLI at week 8 is a complete, demonstrable thing.
The WASM shell is the nicest part of the story but not the load-bearing part.

## After v0.1 — candidates, not commitments

Ordered by expected value, each requiring an ADR before starting:

1. **Fortnox/Spiris API writer** — same `Ledger`, journals instead of a file.
   The hedge that means this project doesn't depend on displacing anyone.
2. **Kontantmetoden** — a second posting strategy, which opens the enskild firma
   market.
3. **More VAT scenarios** — driven by what real data actually hits, from the
   `ScenarioGap` telemetry hosts collect.
4. **Multi-currency** — with FX gain/loss accounts. Expensive. Only if demanded.
5. **SIE 5** — XML, attachments, XMLDsig signatures. Only when receiving systems
   support it, which as of now few do.
6. **Peppol / EN 16931** — different problem, different deadline. Probably a
   separate project.
