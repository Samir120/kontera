# 01 — Problem and Scope

**Status:** accepted · **Owner:** Samir · **Last reviewed:** 2026-09-03

This is the document that says no. When a feature idea arrives, it gets checked
against the non-goals list here before it gets checked against anything else.

---

## 1. Problem statement

Swedish e-commerce bookkeeping breaks at the gap between **orders** and
**payouts**.

An order is a customer buying something at a moment in time, with a VAT
treatment determined by what was sold and who bought it. A payout is a payment
provider transferring a *net* amount days later, aggregating many orders,
minus fees, minus refunds, plus adjustments.

The bank statement only ever shows the payout. Producing correct books from that
requires reconstructing everything the payout netted together — and there is no
automatic check that the reconstruction is right.

### 1.1 Worked example

40 orders on 2026-01-05, 50,000 kr incl. 25% moms. Refunds 1,000 kr incl. moms.
Stripe fees 1,168 kr. Payout of 47,832 kr lands 2026-01-08.

Correct postings (default account mapping, see [02](02-domain-model.md)):

| Date | Konto | Debet | Kredit |
|---|---|---:|---:|
| 01-05 | 1580 Fordran betalningsleverantör | 50 000.00 | |
| 01-05 | 3001 Försäljning inom Sverige, 25 % moms | | 40 000.00 |
| 01-05 | 2611 Utgående moms, 25 % | | 10 000.00 |
| 01-05 | 3001 (retur) | 800.00 | |
| 01-05 | 2611 (retur) | 200.00 | |
| 01-05 | 1580 | | 1 000.00 |
| 01-08 | 6570 Betalningsleverantörsavgifter | 1 168.00 | |
| 01-08 | 1580 | | 1 168.00 |
| 01-08 | 1930 Företagskonto | 47 832.00 | |
| 01-08 | 1580 | | 47 832.00 |

After the payout, account 1580 must be **exactly zero** for the covered event
set. That zero is the invariant this library exists to enforce.

### 1.2 The three failure modes

| Failure | Consequence |
|---|---|
| Payout booked as revenue | Revenue overstated by fees, output VAT understated (~1 600 kr in the example above) |
| Sales and cash split across a period boundary | Wrong momsdeklaration for both periods |
| No reconciliation check | Errors accumulate silently until manual year-end cleanup |

### 1.3 Why the market hasn't solved this *for platform builders*

It is solved for **merchants**: A2X, Link My Books and Synder summarise
settlements into journal entries pushed into Xero or QuickBooks. In Sweden,
Fortnox's own WooCommerce connector and third parties (Standout, Redlight,
Wetail) push orders into Fortnox.

It is not solved for **platforms**. If you are building a Swedish webshop and
want correct bookkeeping to exist *inside your product*, your options today are:

1. Send merchants to Fortnox and write a connector — the bookkeeping brain lives
   elsewhere, and there is a Fortnox integration licence on top.
2. Implement Swedish accounting yourself from scratch.

There is no library to install. Not in Rust, not in TypeScript, not anywhere.
Ledger primitives exist (Formance, TigerBeetle, Blnk) but know nothing about
moms, BAS or SIE. Swedish knowledge exists but only inside closed commercial
platforms.

## 2. Users

| User | Role | What they need |
|---|---|---|
| **Platform developer** (primary) | Integrates the library | A typed API, a stable event schema, no runtime dependencies to operate |
| **Merchant** (indirect) | Uses the host's admin panel | Books that tie out; a file to hand to their accountant |
| **Accountant** (gatekeeper) | Receives the output | A SIE 4I file that imports cleanly and needs no correction |

The accountant is the gatekeeper. If they reject the output, nothing else about
the project matters.

## 3. In scope (v0.1)

- **Money** — `rust_decimal`, SEK only, öre precision
- **Ledger primitives** — accounts, verifications, balanced transactions,
  verification number series
- **VAT determination** — four scenarios only (see [02](02-domain-model.md) §3)
- **Posting rules** — config-driven event→verification mapping
- **Settlement reconciliation** — the payout invariant, enforced
- **SIE 4I output** — CP437-encoded, importable by commercial systems
- **Three shells** — CLI, WASM/npm, HTTP sidecar

## 4. Non-goals (v0.1)

Each of these is a real feature that a complete product would need, deliberately
excluded so v0.1 can be finished.

| Excluded | Why |
|---|---|
| Persistence / database | The host already stores orders. State is the host's problem. See [ADR-0001](adr/0001-pure-core.md) |
| HTTP server, auth, multi-tenancy | Shell concerns, not engine concerns |
| Invoicing, PDF, customers, products | The host platform owns these |
| Peppol / EN 16931 e-invoicing | Different problem, different deadline (ViDA, 2030) |
| Bank feeds, camt.053, PSD2 | The payout event comes from the payment provider, not the bank |
| Multi-currency | Doubles the type surface, adds FX gain/loss accounts. Hurts to cut. Cut it |
| Kontantmetoden (cash-basis) | v0.1 is accrual only. Cash-basis is a second posting strategy, not a tweak |
| Year-end: depreciation, periodiseringsfond, NE-bilaga | Annual work, not transactional |
| Purchase side / supplier invoices / ingående moms | Sales and settlement only |
| Admin UI | The host builds it. We ship the data |

**Multi-currency is the one that will be tempting.** It is out. Revisit at v0.3
with a written ADR, not in a Sunday-afternoon commit.

## 5. Success criteria

### 5.1 The acceptance test

> A merchant closes a month in the host's admin panel, downloads a SIE 4I file,
> emails it to their accountant, and the accountant imports it into Fortnox
> without touching anything.

Everything else in this repository exists to make that sentence true.

### 5.2 Measurable exit criteria for v0.1

| # | Criterion | Verified by |
|---|---|---|
| S1 | A generated `.si` file imports into a Fortnox or Visma trial account with zero manual correction | Manual, recorded in `docs/acceptance/` |
| S2 | For any generated event sequence, every verification balances | `proptest` |
| S3 | For any permutation of a settled batch, output is byte-identical | `proptest` |
| S4 | The VAT report reconciles to the sum of underlying postings, exactly | `proptest` |
| S5 | A settlement whose components don't sum to the payout returns `Err`, never `Ok` | unit + `proptest` |
| S6 | `kontera-core` has zero I/O, zero async, zero `unsafe` | `cargo-deny` + CI grep + review |
| S7 | SwadeStack produces a month's SIE file via the npm package | Integration branch |
| S8 | Swedish characters (`å ä ö`) survive the CP437 round-trip | Unit + golden file |

### 5.3 Explicit anti-goals for v0.1

Not measured, not optimised, not discussed: throughput, latency, memory
footprint, concurrent access, horizontal scaling. This is a fold over an event
log that runs once a day on a few thousand events.

## 6. Regulatory posture

This library produces accounting data used in filings to Skatteverket. That
carries real responsibility.

**Rules:**

1. Every regulatory claim in the codebase or docs carries a source reference
   (BFL chapter/§, Mervärdesskattelagen, BFN allmänna råd, BAS-kontoplanen year).
2. Default mappings are marked **VERIFY** until confirmed against the current
   BAS chart and Skatteverket guidance by a qualified person.
3. The library **fails closed** — unknown scenarios return `Err`. See
   [ADR-0004](adr/0004-fail-closed.md).
4. The README carries a no-warranty disclaimer, and the licence file will too.
5. Before any public release, one Swedish accountant reviews the default chart
   mapping and the VAT scenario table.

Reference points relevant to design:

- Bokföringslagen requires an unbroken verification number series and traceable
  corrections. Posted verifications are immutable; corrections are reversing
  entries, never edits.
- Since 2024-07-01, paper originals may be destroyed once accounting information
  has been transferred to another form without risk of alteration or loss. This
  makes integrity of digital records a technical requirement, not just good
  practice.
- Räkenskapsinformation must be retained seven years after the end of the
  calendar year in which the financial year closed. The library does not store
  anything — retention is the host's obligation, and this must be stated in the
  integration guide.

## 7. Open questions

| # | Question | Needed by |
|---|---|---|
| Q1 | Which BAS account for payment provider receivables — 1580, or a 158x subaccount per provider? | Week 3 |
| Q2 | Do returns reverse against the original sales account, or a dedicated returns account? | Week 3 |
| Q3 | Which momsdeklaration boxes must v0.1 populate? | Week 4 |
| Q4 | Does Fortnox's SIE import accept `#VER` series other than `"A"`? | Week 6 |
| Q5 | Licence: permissive for adoption, or copyleft to block SaaS competitors? | Before first publish |
