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
tagged. `EVENT_SCHEMA_VERSION` is `1`. Every object is strict: an unknown field
is an error, never dropped ([ADR-0006](adr/0006-oss-charged-percentage.md) §4).

```rust
pub const EVENT_SCHEMA_VERSION: u32 = 1;

// Identifiers. Boundary parsers, strict on shape and nothing else.
pub struct EventId(String);        // host-assigned, globally unique, stable; non-empty
pub struct ProviderId(String);     // lowercase TOML bare key, [a-z0-9_-]+ — it is the config key
pub struct CountryCode([u8; 2]);   // ISO 3166-1 alpha-2 *shape*: two ASCII capitals, not a membership test
pub struct VatNumber(String);      // non-empty; VIES validation is the host's (§2.1)

#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    SaleCaptured(SaleCaptured),    // each variant wraps its own struct, so a rule can
    RefundIssued(RefundIssued),    // take a `&SaleCaptured` without re-matching
    FeeCharged(FeeCharged),
    PayoutSettled(PayoutSettled),
}

pub struct SaleCaptured {
    pub id: EventId,
    pub at: NaiveDate,
    pub provider: ProviderId,
    pub buyer: BuyerTaxStatus,
    pub ship_to: CountryCode,
    pub lines: Vec<SaleLine>,
}
pub struct RefundIssued {
    pub id: EventId,
    pub at: NaiveDate,
    pub original_sale: EventId,
    pub lines: Vec<SaleLine>,          // partial refunds need lines, not a total
}
pub struct FeeCharged {
    pub id: EventId,
    pub at: NaiveDate,
    pub provider: ProviderId,
    pub kind: FeeKind,                 // Transaction | Payout | Dispute | Other
    amount: Money,                     // private: strictly positive, via FeeCharged::new
}
pub struct PayoutSettled {
    pub id: EventId,
    pub at: NaiveDate,
    pub provider: ProviderId,
    pub amount: Money,                 // net, as it hits the bank; may be negative
    pub covers: Vec<EventId>,          // explicit. never inferred
}

pub struct SaleLine {
    pub description: String,
    amount_excl_vat: Money,            // private: non-negative, via SaleLine::new
    pub vat_rate: VatRate,             // Standard | Reduced12 | Reduced6 | Zero | Destination(VatPercent)
    pub goods_or_services: SupplyKind, // Goods | Services
}

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuyerTaxStatus {
    Consumer,
    Business { vat_number: VatNumber },  // caller validates against VIES, not us
}
```

Enum wire forms are `snake_case` and match the config keys in §3 exactly:
`"standard"`, `"reduced12"`, `"reduced6"`, `"zero"`. These Swedish classes carry
no percentage — the number comes from `[vat.rates.se]` and is applied in
`vat.rs`, by `line_vat`.

The host picks the class for each line. Kontera can't tell food from anything
else, so a line sent in the wrong class posts without an error. Goods also move
between classes. Food is `reduced6` from 1 April 2026 through 31 December 2027
and `reduced12` before and after, depending on when the taxable event occurs
(prop. 2025/26:55;
[Skatteverket](https://www.skatteverket.se/foretag/moms/saljavarorochtjanster/momssatserochundantagfranmoms),
page modified 2026-11-02).

> **VERIFY (Q11):** a refund line carries the same `vat_rate` as the sale line it
> reverses ([ADR-0006](adr/0006-oss-charged-percentage.md) §1), so a refund after
> 1 April of a March food sale carries `reduced12`. That is this project's rule.
> Prop. 2025/26:55 doesn't appear to address credit notes or returns across the
> change. Accountant review has the final word.

An OSS line has no Swedish class. It carries the percentage the checkout charged:
`{"destination":"19.00"}`. A line holds a class or a percentage, never both.
Which is valid depends on the sale's scenario, which `vat.rs` decides
([ADR-0006](adr/0006-oss-charged-percentage.md) §1).

### 2.1 Design notes

- **`amount_excl_vat`, not `amount_incl_vat`.** Incl-VAT input forces the library
  to back out VAT by division, which introduces rounding the host's checkout
  didn't do. The host knows its own net figures; it passes them.
- **Two sign constraints live in the types.** A sale line is never negative (a
  negative line is a refund, and refunds are their own event); a fee is strictly
  positive. Each is the one private field of its struct, so the constructor is
  the only way in and JSON deserialisation goes through it. An empty `lines`
  on a sale or refund is *not* a type-level rejection — it needs the event id
  for a useful message, which is the `validate` stage's job ([03 §4.1](03-architecture.md#41-pipeline-stages)).
- **`covers` is explicit.** The host has the provider's settlement report. The
  library does not guess which events a payout covers — guessing is precisely
  where these systems fail.
- **`ship_to` names the VAT territory, not the postal country.** The scenario
  depends on the EU VAT area, which is not the list of EU member states
  ([02 §3.1](02-domain-model.md#31-eu-means-the-eu-vat-area)). Send `XI` for
  Northern Ireland and `GB` for the rest of the United Kingdom. Send a
  territory's own code where it has one, such as `AX` for Åland. A territory
  outside the VAT area with no code of its own, such as the Canary Islands, must
  not be sent under its member state's code, and v0.1 has no way to express a
  sale to it. Two letters can't tell Kontera any of this, so the host answers
  for it. A code that is neither in the VAT area nor an assigned ISO 3166-1
  code is refused with `UnknownShipTo` (§4) and never booked as an export.
  `XK`, `IC`, `EA`, `UK` and `EL` are such codes.
- **A service sold outside Sweden is refused.** The four scenarios are written
  for goods, and where a service is taxed follows other rules that v0.1 does
  not model. A `services` line on a sale with `ship_to` other than `SE` is
  `UnsupportedScenario` with `CrossBorderServices` (§4), whoever the buyer is
  and whatever rate the line carries. A service sold within Sweden posts like
  goods.
- **VAT number is not validated here.** VIES lookup is network I/O. The host does
  it and passes the result. The library trusts the caller's assertion and
  records it.
- **`ProviderId` is case-sensitive and lowercase.** It is the lookup key into
  `[accounts.receivable]`, and case is the one way `"Stripe"` and `stripe` can
  disagree while looking identical.
- **No customer, product or address data.** Only what affects posting. Less PII
  crossing the boundary is a feature.

> **Q6 (open):** §7 says the schema version travels "in the payload", but the
> JSONL in §2.2 has no `schema_version` field. Per-line field, a header line,
> or out-of-band — decide in ADR-0005 before M5, when a host first persists
> events.

### 2.2 Example

```jsonl
{"type":"sale_captured","id":"ord_1001","at":"2026-01-05","provider":"stripe","buyer":{"kind":"consumer"},"ship_to":"SE","lines":[{"description":"Widget","amount_excl_vat":"1000.00","vat_rate":"standard","goods_or_services":"goods"}]}
{"type":"sale_captured","id":"ord_1002","at":"2026-01-06","provider":"stripe","buyer":{"kind":"consumer"},"ship_to":"DE","lines":[{"description":"Widget","amount_excl_vat":"1000.00","vat_rate":{"destination":"19.00"},"goods_or_services":"goods"}]}
{"type":"fee_charged","id":"fee_2026_01_08","at":"2026-01-08","provider":"stripe","kind":"transaction","amount":"1168.00"}
{"type":"payout_settled","id":"po_9001","at":"2026-01-08","provider":"stripe","amount":"47832.00","covers":["ord_1001","fee_2026_01_08"]}
```

The second sale ships to Germany and carries the rate its checkout charged. This
block is parsed by a test (`kontera-testkit/tests/docs_04_event_example.rs`), so
it cannot drift from the types.

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
# Months already filed. Events dated inside these are rejected.
closed_periods = ["2026-01"]

[series]
default      = "A"
start_number = 1

# Every account number below is a default from 02 §4, and every one is
# VERIFY (Q8): confirm it against the current BAS chart before relying on it.
[accounts]
bank         = "1930"
provider_fee = "6570"

[accounts.receivable]            # per provider — see Q1 in 02 §4.1
stripe = "1580"
klarna = "1581"

[accounts.sales]
eu_b2b_reverse    = "3108"
export_outside_eu = "3105"
# eu_b2c_oss      =              # Q8: no account confirmed yet; leave unset

[accounts.sales.se]
standard  = "3001"
reduced12 = "3002"
reduced6  = "3003"

[accounts.vat_payable]
# eu_b2c_oss =                   # Q8: no account confirmed yet; leave unset

[accounts.vat_payable.se]
standard  = "2611"
reduced12 = "2621"
reduced6  = "2631"

[returns]
strategy = "reverse_original"    # the only strategy until Q2 is answered

# Host-updatable; rates change. Source: Skatteverket, "Momssatser och undantag
# från moms", page modified 2026-11-02.
[vat.rates.se]
standard  = "25.00"
reduced12 = "12.00"
reduced6  = "6.00"

# One line per country you sell to, keyed like `ship_to`. An example, not data.
# VERIFY: Germany's standard and reduced rates, noted 2026-11-17 and not read
# from a primary source (UStG § 12). Keep your own table current.
[vat.rates.oss]
DE = ["19.00", "7.00"]
```

This block is parsed by a test (`kontera-testkit/tests/docs_04_config_example.rs`),
so it cannot drift from the types.

**Required:** `schema_version` (must be `1`), `[company]`, `[fiscal_year]`,
`[series]`, `accounts.bank`, `accounts.provider_fee`, `[accounts.receivable]`
(it may be empty), and all three keys of `[vat.rates.se]`. **Optional:**
`closed_periods`, every key under `[accounts.sales]` and
`[accounts.vat_payable]`, `[returns]`, which defaults to `reverse_original`, and
`[vat.rates.oss]`.

**Strict.** An unknown key at any level is an error, so a typo is never a
silent `None`. Account numbers and rates are strings only: `bank = 1930` and
`standard = 25` are rejected, because a TOML number is the wrong type for an
identifier and a float for a rate (ADR-0002). The `[*.se]` keys are the
`VatRate` wire forms from §2. `zero` has no key: a zero-rated line takes its
accounts from its scenario.

**`[vat.rates.oss]`** lists the rates valid for each destination country
([ADR-0006](adr/0006-oss-charged-percentage.md) §3). It validates and never
supplies: the number posted is always the one on the line, so this table can
refuse a posting but cannot change one. A country not listed has no valid rates.
A key must be a country an OSS sale can ship to: in the EU VAT area
([02 §3.1](02-domain-model.md#31-eu-means-the-eu-vat-area)) and not `SE`.
Anything else, such as `NO`, `GB` or a mistyped `XX`, is
`ConfigError::NotAnOssDestination`, because its rates could never be used.
Northern Ireland is `XI`.
A rate listed twice, `["20", "20.00"]` included, is
`ConfigError::DuplicateOssRate`. Zero is not a valid rate: a destination zero
rate is a `ScenarioGap`. The host maintains this table, and the library ships no
rate data.

**Checked when the config is read**, before `post()` runs:

| Key | Must be in |
|---|---|
| `bank`, every `receivable.*` | class 1 |
| every `vat_payable.*` | class 2 |
| every `sales.*` | class 3 |
| `provider_fee` | classes 4–7 |

`bank` must also differ from every receivable: a payout moves money from the
clearing account to the bank, and the clearing account cannot net to zero (I3)
if both are the same account. A violation is `ConfigError::WrongAccountClass`
or `ConfigError::BankIsReceivable`, naming the key.

A missing mapping for a scenario the events require is
`Err(MissingAccountMapping)` at validation time, not a silent fallback. Leaving
a key out is how a merchant who never sells abroad says so; a placeholder
account would post real money to the wrong place.

## 4. Error taxonomy

Errors are the product. Each must say what was wrong, which event, and what a
human should do.

`PostingError` is the one error type `post()` returns. It has two kinds of
variant: **nesting** variants that wrap a module's own error enum via
`#[from]` and forward `Display`/`source()` transparently, and **leaf**
variants that carry the data of a failure only the fold can detect.

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PostingError {
    // ── nesting: one per module that has its own error enum ──────────────
    #[error(transparent)]
    Money(#[from] MoneyError),                 // Syntax | TooPrecise | OutOfRange
    #[error(transparent)]
    Account(#[from] AccountError),             // Syntax | OutOfRange
    #[error(transparent)]
    Verification(#[from] VerificationError),   // Unbalanced | Empty | InvalidSeries
    #[error(transparent)]
    Event(#[from] EventError),                 // Invalid{EventId,ProviderId,CountryCode,VatNumber}
                                               // | NegativeLineAmount | NonPositiveFee

    // ── leaf: these four exist ───────────────────────────────────────────
    #[error("event {event} ships to `{ship_to}`, which is neither in the EU VAT \
             area nor an assigned ISO 3166-1 country code; send the code of the \
             VAT territory the goods ship to")]
    UnknownShipTo { event: EventId, ship_to: CountryCode },   // returned by vat::classify

    // The next three are returned by vat::check_sale.
    #[error("unsupported VAT scenario for event {event}: {reason}; v0.1 cannot \
             post it, so book this order by hand")]
    UnsupportedScenario { event: EventId, reason: ScenarioGap },

    #[error("event {event}, lines[{line}]: vat_rate `{vat_rate}` does not fit a \
             {scenario} sale; SeDomestic takes a Swedish class, EuB2cOss a \
             destination percentage, EuB2bReverse and ExportOutsideEu `zero`")]
    VatRateMismatch { event: EventId, line: usize, scenario: VatScenario, vat_rate: VatRate },

    #[error("event {event}, lines[{line}]: {percent} % is not listed for \
             `{country}` in [vat.rates.oss]; correct the line, or add the rate \
             if the checkout charged it")]
    UnlistedOssRate { event: EventId, line: usize, country: CountryCode, percent: VatPercent },

    // ── leaf: still to come, each with the code that first returns it ────
    #[error("settlement {payout} does not reconcile: expected {expected}, \
             provider reported {actual}, difference {delta}")]
    SettlementMismatch {
        payout: EventId, expected: Money, actual: Money, delta: Money,
    },

    #[error("event {event} references unknown event {missing}")]
    UnknownReference { event: EventId, missing: EventId },

    #[error("duplicate event id {0}")]
    DuplicateEventId(EventId),

    #[error("no account configured for {key}")]
    MissingAccountMapping { key: MappingKey },

    #[error("event {event} dated {at} falls in closed period {period}")]
    PeriodClosed { event: EventId, at: NaiveDate, period: Month },

    #[error("event {event} dated {at} is outside fiscal year {start}–{end}")]
    OutsidePeriod { event: EventId, at: NaiveDate, start: NaiveDate, end: NaiveDate },
}

// The `reason` of `UnsupportedScenario`. It has a `Display` and is not an error.
pub enum ScenarioGap {
    CrossBorderServices,   // "a service sold outside Sweden"
    DestinationZeroRate,   // "a zero-rated line on an OSS sale"
    DomesticZeroRate,      // "a line without VAT on a domestic sale"
}
```

The nesting variants exist as of M1. Each leaf variant lands together with the
code that first returns it and a test that produces it — a variant nothing
constructs is untestable beyond its message. Four leaves exist. `classify` in
`vat.rs` returns `UnknownShipTo`, and `check_sale` beside it returns
`UnsupportedScenario`, `VatRateMismatch` and `UnlistedOssRate`. `Unbalanced`
lives in `VerificationError`, not at the top level: the private constructor is
where it is detected, so that is where it is defined.

Two error enums sit outside `PostingError`: `ConfigError` (schema version,
fiscal year, months, org number, account roles, OSS rates — §3) and `VatError`
(`VatPercent`). Both are raised while the host deserialises its config,
before `post()` is called — `post()` takes an already-proven `&Config` — so
no path through `post()` produces them, and nesting them would add variants
nothing can construct. Through serde they surface as the deserialiser's error,
carrying the same message; a host calling `FiscalYear::new` or
`VatPercent::parse` directly gets the typed value. If a later `post()` stage
ever raises a variant of either, that enum nests then.

`SettlementMismatch` carries `delta` deliberately — that number is the first
thing a human needs, and it is usually a fee the host forgot to emit.

`ScenarioGap` is an enum, not a string. It turns the v0.1 non-goals into
machine-readable output, so the host can tell the merchant exactly which orders
need manual handling instead of failing opaquely. It is plain data and not an
error type: a gap needs the event id beside it, so `UnsupportedScenario`
carries it as a field. Three gaps exist, all returned by `check_sale`:

| `ScenarioGap` | Returned for |
|---|---|
| `CrossBorderServices` | A `services` line on a sale with `ship_to` other than `SE`, in any scenario |
| `DestinationZeroRate` | A `zero` line on an `EuB2cOss` sale ([ADR-0006](adr/0006-oss-charged-percentage.md) §2) |
| `DomesticZeroRate` | A `zero` line on an `SeDomestic` sale |

The other exclusions of [02 §3](02-domain-model.md#3-vat-scenarios-v01) have no
variant yet: a VAT-exempt merchant, a gift card or voucher, construction
reverse charge, a margin scheme, a prepayment whose VAT point differs from
delivery, and a non-EU special scheme. The same goes for a second currency,
which is a non-goal of [01 §4](01-problem-and-scope.md#4-non-goals-v01). No
event field reveals any of them, so nothing could return such a variant or test
it. Each arrives as a variant together with the schema field that makes it
detectable. Until then the library can't refuse such an order, and the host
must not send one as an ordinary sale.

Only an unsupported treatment is a gap. A line whose `vat_rate` its scenario
can't carry is `VatRateMismatch`, and a destination percentage that
`[vat.rates.oss]` doesn't list for the sale's `ship_to` is `UnlistedOssRate`.
The first is an inconsistent event and the second a config or data mismatch.
They are separate leaves because reporting them to the merchant as "handle
manually" would hide platform bugs
([ADR-0006](adr/0006-oss-charged-percentage.md) §1 has the table). Both carry
`line`, the index into the sale's `lines` counted from zero, and print it as
`lines[1]`. A gap names only the event, because the merchant handles the whole
order.

`check_sale` returns one failure for a sale, and always the same one. An
unknown `ship_to` is reported before any line. The lines are then checked in
order, and within a line the services check comes before the rate table. A
sale with no lines passes this check and is refused later (§2.1).

`UnknownShipTo` is a leaf of the same kind as the mismatch. A `ship_to` that is
neither in the EU VAT area nor an assigned ISO 3166-1 code
([02 §3.1](02-domain-model.md#31-eu-means-the-eu-vat-area)) is a host mistake
or a territory v0.1 can't express. Booking it as an export would post a sale
with no VAT on the strength of a typo, so `classify` refuses it.

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

**Both schemas freeze at the first published release that can parse events**; a
stub publish reserving the name doesn't count. Until then, changing v1 takes an
ADR and a `schema:` commit. After it, every wire addition is a bump, because
strict readers refuse it ([ADR-0006](adr/0006-oss-charged-percentage.md) §5).

Pre-1.0, the Rust API may change freely. The event schema may not — hosts have
persisted events on disk, and they are räkenskapsinformation subject to seven-year
retention. **v1 events must remain readable for seven years.** This is a legal
constraint on the schema, not a courtesy.
