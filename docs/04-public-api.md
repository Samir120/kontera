# 04 — Public API and Contracts

**Status:** draft · **Owner:** Samir · **Last reviewed:** 2026-09-03

Three contracts, each versioned independently of the crate: the **event
schema** (input), the **config schema** (input), and the **SIE output**.

---

## 1. Entry points

```rust
/// Fold an event log into a ledger. Pure.
pub fn post(events: &[Event], config: &Config) -> Result<Ledger, PostingError>;

/// Serialise a ledger as SIE 4I. `generated_at` is passed in, never read
/// from the system clock — determinism depends on it.
pub fn write_4i(ledger: &Ledger, generated_at: NaiveDate) -> Result<Vec<u8>, SieError>;
```

`write_4i` returns `Vec<u8>`, not `String`, because the output is CP437 and not
valid UTF-8. See [ADR-0003](adr/0003-sie-4i-output.md).

## 2. Event schema v1

Serde-compatible. JSON Lines on the wire; `#[serde(tag = "type")]` internally
tagged.

```rust
pub struct EventId(pub String);   // host-assigned, globally unique, stable

#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    SaleCaptured {
        id: EventId,
        at: NaiveDate,
        provider: ProviderId,
        buyer: BuyerTaxStatus,
        ship_to: CountryCode,          // ISO 3166-1 alpha-2
        lines: Vec<SaleLine>,
    },
    RefundIssued {
        id: EventId,
        at: NaiveDate,
        original_sale: EventId,
        lines: Vec<SaleLine>,          // partial refunds need lines, not a total
    },
    FeeCharged {
        id: EventId,
        at: NaiveDate,
        provider: ProviderId,
        kind: FeeKind,                 // Transaction | Payout | Dispute | Other
        amount: Money,                 // positive
    },
    PayoutSettled {
        id: EventId,
        at: NaiveDate,
        provider: ProviderId,
        amount: Money,                 // net, as it hits the bank
        covers: Vec<EventId>,          // explicit. never inferred
    },
}

pub struct SaleLine {
    pub description: String,
    pub amount_excl_vat: Money,
    pub vat_rate: VatRate,             // Standard | Reduced12 | Reduced6 | Zero
    pub goods_or_services: SupplyKind, // affects EU treatment
}

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuyerTaxStatus {
    Consumer,
    Business { vat_number: String },   // caller validates against VIES, not us
}
```

### 2.1 Design notes

- **`amount_excl_vat`, not `amount_incl_vat`.** Incl-VAT input forces the library
  to back out VAT by division, which introduces rounding the host's checkout
  didn't do. The host knows its own net figures; it passes them.
- **`covers` is explicit.** The host has the provider's settlement report. The
  library does not guess which events a payout covers — guessing is precisely
  where these systems fail.
- **VAT number is not validated here.** VIES lookup is network I/O. The host does
  it and passes the result. The library trusts the caller's assertion and
  records it.
- **No customer, product or address data.** Only what affects posting. Less PII
  crossing the boundary is a feature.

### 2.2 Example

```jsonl
{"type":"sale_captured","id":"ord_1001","at":"2026-01-05","provider":"stripe","buyer":{"kind":"consumer"},"ship_to":"SE","lines":[{"description":"Widget","amount_excl_vat":"1000.00","vat_rate":"standard","goods_or_services":"goods"}]}
{"type":"fee_charged","id":"fee_2026_01_08","at":"2026-01-08","provider":"stripe","kind":"transaction","amount":"1168.00"}
{"type":"payout_settled","id":"po_9001","at":"2026-01-08","provider":"stripe","amount":"47832.00","covers":["ord_1001","fee_2026_01_08"]}
```

## 3. Config schema v1 (TOML)

```toml
schema_version = 1

# Example values only — clearly fictional. Config examples get copy-pasted,
# so no real company or org number appears anywhere in this repository.
[company]
org_number = "XXXXXX-XXXX"
name       = "Exempelbutiken AB"

[fiscal_year]
start = "2026-01-01"
end   = "2026-12-31"

# Periods already filed. Events dated inside these are rejected.
closed_periods = ["2026-01"]

[series]
default = "A"
start_number = 1

[accounts]
bank         = "1930"
provider_fee = "6570"

[accounts.receivable]        # per provider — see Q1 in 01
stripe = "1580"
klarna = "1581"

[accounts.sales.se]
standard  = "3001"
reduced12 = "3002"
reduced6  = "3003"

[accounts.sales.eu_b2b_reverse]  = "3106"
[accounts.sales.export_outside_eu] = "3105"

[accounts.vat_payable]
standard  = "2611"
reduced12 = "2612"
reduced6  = "2613"

[returns]
strategy = "reverse_original"    # or "dedicated_account"

[vat.rates]                       # host-updatable; rates change
se_standard = "25.00"
se_reduced12 = "12.00"
se_reduced6  = "6.00"
```

Missing mapping for a scenario the events require → `Err(MissingAccountMapping)`
at validation time, not a silent fallback.

## 4. Error taxonomy

Errors are the product. Each must say what was wrong, which event, and what a
human should do.

```rust
#[derive(Debug, thiserror::Error)]
pub enum PostingError {
    #[error("verification would not balance: debit {debit}, credit {credit}")]
    Unbalanced { debit: Money, credit: Money },

    #[error("settlement {payout} does not reconcile: expected {expected}, \
             provider reported {actual}, difference {delta}")]
    SettlementMismatch {
        payout: EventId, expected: Money, actual: Money, delta: Money,
    },

    #[error("unsupported VAT scenario for event {event}: {reason}")]
    UnsupportedScenario { event: EventId, reason: ScenarioGap },

    #[error("event {event} references unknown event {missing}")]
    UnknownReference { event: EventId, missing: EventId },

    #[error("duplicate event id {0}")]
    DuplicateEventId(EventId),

    #[error("no account configured for {key}")]
    MissingAccountMapping { key: MappingKey },

    #[error("event {event} dated {at} falls in closed period {period}")]
    PeriodClosed { event: EventId, at: NaiveDate, period: String },

    #[error("event {event} dated {at} is outside fiscal year {start}–{end}")]
    OutsidePeriod { event: EventId, at: NaiveDate, start: NaiveDate, end: NaiveDate },
}
```

`SettlementMismatch` carries `delta` deliberately — that number is the first
thing a human needs, and it is usually a fee the host forgot to emit.

`ScenarioGap` is an enum, not a string: `VatExemptMerchant`, `GiftCard`,
`ConstructionReverseCharge`, `PrepaymentVatPoint`, `NonEuNonSeSpecialScheme`,
`MultiCurrency`. This turns the v0.1 non-goals into machine-readable output —
the host can tell the merchant exactly which orders need manual handling instead
of failing opaquely.

**`post()` never panics on any input.** Fuzz-tested. See
[05](05-testing-strategy.md).

## 5. Output: `Ledger`

```rust
pub struct Ledger {
    pub company:       CompanyInfo,
    pub fiscal_year:   DateRange,
    pub verifications: Vec<Verification>,   // sorted, numbered, unbroken
    pub vat_report:    VatReport,
    pub reconciliation: Vec<SettlementSummary>,
}
```

`Ledger` is `serde`-serialisable so a host can render a huvudbok or momsrapport
without going through SIE. `SettlementSummary` records each payout with its
components and the resulting zero balance — this is the audit artifact a merchant
shows their accountant.

## 6. SIE 4I output contract

**Type 4I, not 4E.** 4I is for a *försystem* generating bookkeeping orders for
import into an accounting system, containing verifications only. That is exactly
this library's role. 4E is a full export with balances and belongs to whoever
owns the ledger. File extension `.si`.

Encoding **CP437** (IBM PC-8), per the SIE specification. This is the single most
likely source of a week-6 acceptance failure: everything works locally until the
first `å` in a company name.

Header records emitted: `#FLAGGA`, `#PROGRAM`, `#FORMAT`, `#GEN`, `#SIETYP`,
`#ORGNR`, `#FNAMN`, `#RAR`, `#KONTO`.

Verification form:

```
#VER "A" "1" 20260105 "Försäljning 2026-01-05" 20260108
{
   #TRANS 1580 {} 50000.00
   #TRANS 3001 {} -40000.00
   #TRANS 2611 {} -10000.00
}
```

Amounts: debit positive, credit negative, two decimals, `.` separator. Dates
`YYYYMMDD`.

> **VERIFY (Q4):** whether target systems accept series letters other than `"A"`,
> and how they behave on `#KONTO` records for accounts that already exist in the
> receiving chart. Test against a Fortnox trial in week 6.

## 7. Stability policy

| Contract | Versioned by | Breaking change means |
|---|---|---|
| Event schema | `schema_version` in the payload | New major schema version; library supports N and N−1 |
| Config schema | `schema_version` in the TOML | Same |
| Rust API | SemVer on the crate | Normal Rust rules |
| SIE output | The SIE spec | Not ours to break |

**The event schema version is independent of the crate version.** A patch release
must never change the schema; a schema bump does not force a major crate bump if
the Rust API is unchanged. See [ADR-0005](adr/0005-event-schema-versioning.md).

Pre-1.0, the Rust API may change freely. The event schema may not — hosts have
persisted events on disk, and they are räkenskapsinformation subject to seven-year
retention. **v1 events must remain readable for seven years.** This is a legal
constraint on the schema, not a courtesy.
