# 02 — Domain Model

**Status:** draft · **Owner:** Samir · **Last reviewed:** 2026-09-03

The hardest knowledge in this project is not the Rust. It is this document.
Everything marked **VERIFY** must be confirmed against the current BAS chart,
Mervärdesskattelagen and Skatteverket guidance before release.

---

## 1. Glossary

Swedish terms are used in code where they are the domain's real name. Anything
in this table may appear as an identifier.

| Swedish | English | Notes |
|---|---|---|
| bokföring | bookkeeping | |
| verifikation | verification | One balanced journal entry. Immutable once posted |
| verifikationsnummer | verification number | Unbroken sequential series, required by BFL |
| verifikationsserie | verification series | A letter, e.g. `A`. Multiple series allowed |
| konto | account | Four-digit BAS number |
| kontoplan | chart of accounts | BAS-kontoplanen is the Swedish standard |
| debet / kredit | debit / credit | |
| moms (mervärdesskatt) | VAT | 25% standard, 12% and 6% reduced |
| utgående moms | output VAT | VAT charged on sales |
| ingående moms | input VAT | VAT paid on purchases — **out of scope v0.1** |
| momsdeklaration | VAT return | Filed monthly, quarterly or annually |
| omvänd skattskyldighet | reverse charge | Buyer accounts for VAT |
| periodisk sammanställning | EC sales list | For EU B2B — **out of scope v0.1** |
| OSS | One Stop Shop | EU B2C distance selling scheme |
| räkenskapsår | financial year | |
| räkenskapsinformation | accounting information | Subject to 7-year retention |
| rättelseverifikat | correcting entry | Reversal, never an edit |
| avräkningskonto | clearing/settlement account | Where provider receivables sit |
| SIE | Standard Import Export | Swedish accounting interchange format |
| BFL | Bokföringslagen | The Accounting Act |
| ML | Mervärdesskattelagen (2023:200) | The VAT Act |
| BFN | Bokföringsnämnden | Swedish Accounting Standards Board |

## 2. Core entities

### 2.1 Money

`Money` wraps `rust_decimal::Decimal`. Never `f64`. See
[ADR-0002](adr/0002-decimal-money.md).

- Scale: 2 (öre)
- Currency: SEK only in v0.1, but the type carries a currency tag so adding
  others later is a change in one place rather than everywhere
- Rounding: half-away-from-zero at the line level, then summed. Never sum then
  round — the two differ and only one matches what a merchant's checkout did

### 2.2 Account

```
Account { number: AccountNumber, kind: AccountKind }
AccountNumber: 4 digits, 1000–8999
AccountKind:   Asset | Liability | Equity | Income | Expense
```

BAS classes: 1xxx assets, 2xxx liabilities and equity, 3xxx income,
4xxx–7xxx expenses, 8xxx financial items.

### 2.3 Verification and BalancedTransaction

```
Verification {
    series:  Series,            // 'A'
    number:  u32,               // sequential, unbroken
    date:    NaiveDate,         // transaction date
    text:    String,            // description
    posted:  NaiveDate,         // when compiled
    lines:   Vec<Line>,
}
Line { account: AccountNumber, amount: Money }  // debit +, credit −
```

**Invariant:** `lines.iter().map(|l| l.amount).sum() == Money::ZERO`.

This is enforced by a private constructor on `BalancedTransaction` that returns
`Err` unless the sum is zero. An unbalanced verification cannot exist as a value
in this system. That single design choice is the reason for the type system.

### 2.4 Immutability

A posted verification is never modified. Corrections are new verifications that
reverse and re-post. BFL requires traceability, and this also makes the whole
pipeline a pure fold with no update path.

## 3. VAT scenarios (v0.1)

Exactly four. Anything else returns `Err(UnsupportedScenario)`.

| ID | Scenario | Condition | VAT | Buyer status |
|---|---|---|---|---|
| `SeDomestic` | Sale within Sweden | Ship-to SE | 25% / 12% / 6% | Any |
| `EuB2bReverse` | EU business, reverse charge | Ship-to EU ≠ SE, valid VAT no. | 0%, buyer accounts | Business |
| `EuB2cOss` | EU consumer, destination rate | Ship-to EU ≠ SE, no VAT no. | Destination country rate | Consumer |
| `ExportOutsideEu` | Outside EU | Ship-to non-EU | 0% | Any |

**Deliberately excluded from v0.1:** VAT-exempt merchants under the 120,000 kr
turnover threshold, multi-purpose vouchers and gift cards, construction reverse
charge, margin schemes, prepayments where the VAT point differs from delivery,
VOEC (Norway) and UK schemes. Each is a real scenario. Each is `Err` for now.

> **VERIFY:** the EU B2C rate table must come from a maintained source, not be
> hardcoded from memory. Rates change. Consider making the rate table part of
> `Config` so the host can update it without a library release.

> **VERIFY:** OSS has its own turnover threshold (roughly 99,680 kr for
> distance selling to EU consumers) below which Swedish VAT may be applied
> instead. v0.1 assumes the merchant is already registered for OSS and states
> this assumption in the integration guide.

## 4. Default account mapping

All of it configurable. These are defaults, and all are **VERIFY**.

### 4.1 Balance sheet

| Konto | Name | Role |
|---|---|---|
| 1580 | Fordringar för kontokort och kuponger | Payment provider receivable (the clearing account) |
| 1930 | Företagskonto / checkkonto | Bank |
| 2611 | Utgående moms, 25 % | Output VAT standard |
| 2612 | Utgående moms, 12 % | Output VAT reduced |
| 2613 | Utgående moms, 6 % | Output VAT reduced |

> **Q1 (open):** one 1580 for all providers, or `1581 Stripe`, `1582 Klarna`,
> `1583 Swish`? Per-provider subaccounts make reconciliation far easier to read
> and cost nothing. Leaning yes.

### 4.2 Income

| Konto | Name | Scenario |
|---|---|---|
| 3001 | Försäljning inom Sverige, 25 % moms | `SeDomestic` 25% |
| 3002 | Försäljning inom Sverige, 12 % moms | `SeDomestic` 12% |
| 3003 | Försäljning inom Sverige, 6 % moms | `SeDomestic` 6% |
| 3106 | Försäljning varor till annat EU-land, momsfri | `EuB2bReverse` |
| 3105 | Försäljning varor till land utanför EU | `ExportOutsideEu` |
| — | OSS sales accounts | **VERIFY** — BAS has dedicated accounts; confirm current numbers |

### 4.3 Expense

| Konto | Name | Role |
|---|---|---|
| 6570 | Bankkostnader | Payment provider fees |

> **Q2 (open):** returns. Reversing against the original sales account keeps the
> chart small and makes net revenue obvious. A dedicated returns account makes
> return volume visible in the P&L. Default to reversal, make it configurable.

## 5. Event model

Four events. This is the entire input vocabulary of the library.

```
SaleCaptured  { id, at, lines[], buyer_tax_status, ship_to, provider }
RefundIssued  { id, at, original_sale, lines[] }
FeeCharged    { id, at, provider, amount, kind }
PayoutSettled { id, at, provider, amount, covers[] }
```

Notes on modelling choices:

- **`SaleCaptured`, not `OrderPlaced`.** An order that is never paid produces no
  bookkeeping. Capture is the accounting event.
- **`RefundIssued` carries lines, not just an amount.** A partial refund of a
  mixed-VAT order cannot be apportioned correctly from a total alone.
- **`PayoutSettled.covers` is explicit.** The library does not guess which
  events a payout covers. The host, which has the provider's settlement report,
  states it. Guessing is where these systems go wrong.

## 6. Invariants

The library's real product. Each is enforced in code and tested with `proptest`.

| # | Invariant | Enforcement |
|---|---|---|
| **I1** | Every verification balances | Type-level: private constructor |
| **I2** | `payout == Σ sales − Σ refunds − Σ fees` for the covered set | `Err(SettlementMismatch)` |
| **I3** | After a payout, the clearing account nets to zero for that set | Derived from I2, asserted in tests |
| **I4** | Verification numbers are sequential with no gaps | Assigned by the ledger builder |
| **I5** | Output is deterministic — same input, byte-identical output | Golden files |
| **I6** | Output is order-independent for a settled batch | `proptest` permutation |
| **I7** | Every event ID appears at most once | `Err(DuplicateEventId)` |
| **I8** | Output VAT per rate equals the sum of postings on that VAT account | `proptest` |

**I2 is the one that makes this project worth building.** Everything else is
table stakes for a ledger; I2 is what A2X sells and what no library provides.

## 7. Period semantics

- A `FiscalYear` has a start and end date.
- An event dated outside the fiscal year is `Err(OutsidePeriod)`.
- A **closed period** rejects new events: `Err(PeriodClosed)`. Late events must
  be posted to the current open period as corrections, never backdated into a
  period whose momsdeklaration has been filed.
- v0.1 tracks open/closed as config, not state — the host says which periods are
  closed. The library has no memory. See [ADR-0001](adr/0001-pure-core.md).

## 8. Worked reference case

The scenario from [01 §1.1](01-problem-and-scope.md#11-worked-example) is the
canonical fixture. It lives at `fixtures/basic-settlement/` as events in,
expected `.si` out, and is the first golden test written.

## 9. Sources to consult

- BAS-kontoplanen (current year) — <https://www.bas.se>
- Bokföringslagen (1999:1078), esp. ch. 5 (löpande bokföring) and ch. 7 (arkivering)
- Mervärdesskattelagen (2023:200)
- BFN allmänna råd, esp. K1 (BFNAR 2006:1) for sole traders
- SIE format specifications — <https://sie.se/format/>
- Skatteverket guidance on momsdeklaration box mapping
