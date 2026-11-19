# ADR-0006 — OSS sale lines carry the charged percentage

**Status:** Accepted · **Date:** 2026-11-02

## Context

`EuB2cOss` is one of the four v0.1 scenarios (02 §3), taxed at the destination
country's rate. The schema can't express that yet: `SaleLine.vat_rate` is a
Swedish class, and classes don't line up across countries. A table keyed by
class would produce a wrong rate that looks right, so that was rejected on
2026-10-19.

The checkout already knows the percentage it charged, and its figures are
authoritative (04 §2.1). The library needs that number on the line, plus a way
to refuse a stale or mistyped one.

## Decision

### 1. The percentage goes in `vat_rate`

`VatRate` gains one variant, `Destination(VatPercent)`:

```json
"vat_rate": "standard"
"vat_rate": {"destination": "19.00"}
```

`vat_rate` stays required, but an OSS line has no Swedish class. A line holds a
class or a percentage, so "both" and "neither" cannot be represented. Existing
lines are unchanged, byte for byte. An older library refuses the new form with
`unknown variant`, so it fails closed.

The line can't check its own scenario, because that comes from `buyer` and
`ship_to`. `check_sale` in `vat.rs` checks every line of a sale against the
scenario that `classify` gave the sale:

| Scenario | Swedish class | `zero` | `destination` |
|---|---|---|---|
| `SeDomestic` | allowed | gap `DomesticZeroRate` | `VatRateMismatch` |
| `EuB2cOss` | `VatRateMismatch` | gap `DestinationZeroRate` (§2) | allowed if listed, else `UnlistedOssRate` (§3) |
| `EuB2bReverse`, `ExportOutsideEu` | `VatRateMismatch` | allowed | `VatRateMismatch` |

Neither `EuB2bReverse` nor `ExportOutsideEu` charges Swedish VAT (02 §3), so
`zero` is the only rate they take. `zero` on a domestic sale is a gap because
v0.1 has no posting for a Swedish sale without VAT.

One check comes before the table. A `Services` line on a sale that ships
outside Sweden is gap `CrossBorderServices` in every scenario (02 §3). It has
to come first: such a line can rightly carry a Swedish class, and the table
would report that as a mismatch.

These failures have different owners, so they are different errors:

- **Gap: an unsupported treatment.** This is
  `PostingError::UnsupportedScenario`, carrying a `ScenarioGap`: the v0.1
  non-goals in machine-readable form (04 §4). The merchant handles the order
  manually.
- **Mismatch or unlisted rate: an inconsistent event, or a config or data
  mismatch.** These are `PostingError::VatRateMismatch` and
  `PostingError::UnlistedOssRate`, separate typed leaves that name the line,
  because reporting them to the merchant as "handle manually" would hide
  platform bugs.

Lines are checked in order and the first failure is returned, so a sale always
gives the same error.

A refund line carries the same `vat_rate` as the sale line it reverses, whatever
the rate is today. T11 enforces this.

### 2. `VatPercent` stays above zero

`{"destination":"0"}` is a parse error. Zero has one spelling, `VatRate::Zero`,
and an OSS sale carrying it is a `ScenarioGap`.

A destination zero rate is not an OSS sale at 0 %. The Commission's
[OSS guide](https://vat-one-stop-shop.ec.europa.eu/system/files/2021-07/OSS_guidelines_en.pdf)
says exempt supplies "must not be included in the One Stop Shop return", and
that "supplies to which a so-called zero rate is applied lead to the same result
as exempt supplies" (**VERIFY, Q10**). That makes it a different treatment, and
v0.1 has no posting for it, so ADR-0004 applies. Keeping the check also keeps
`[vat.rates.se] standard = "0"` a parse error. Widening later is additive.

### 3. `[vat.rates.oss]` lists valid rates per country

```toml
[vat.rates.oss]
DE = ["19.00", "7.00"]
```

`VatRates` gains `oss: BTreeMap<CountryCode, BTreeSet<VatPercent>>`, optional
and empty by default.

- **It validates and never supplies.** The number posted is always the one on
  the line, so this table can refuse a posting but cannot change one.
- **A country not listed has no valid rates.** The sale fails closed. A merchant
  lists only the countries it sells to.
- **The host maintains it.** The library ships no rate data. The 02 §3 VERIFY
  on a maintained source stands.
- **A repeated rate is rejected.** `["20", "20.00"]` is one value twice, which
  usually means one entry was meant to be a different rate. `FiscalYear`'s
  `DuplicateClosedPeriod` is the precedent.
- **A key no OSS sale can ship to is rejected.** That is any code outside the
  EU VAT area (02 §3.1), and `SE`, since OSS means ship-to ≠ SE (02 §3). Such an
  entry would never be consulted. It is `ConfigError::NotAnOssDestination`,
  raised by `Config::new`. The examples used a fictional `XX` until T9 had the
  sourced table, and now show a real country with rates marked **VERIFY**.
- **It is not date-effective.** A rate stays listed for as long as a sale at
  that rate can still be refunded.

### 4. Events become strict

`deny_unknown_fields` goes on `SaleLineWire`, `SaleCaptured`, `RefundIssued`,
`FeeChargedWire` and `PayoutSettled`. `BuyerTaxStatus` deserialises through a
private wire enum with `Consumer {}`, because serde ignores the attribute on an
internally tagged unit variant (the `ReturnsWire` precedent). A test pins
`{"kind":"consumer","typo":1}`.

§1 already closes the OSS case for older readers. Strictness closes the general
one: a field added later would otherwise be dropped silently by any older
library, and in accounting a dropped field can be a wrong number. Adding
strictness is free now and takes a schema bump after publish.

This fits ADR-0005. N and N−1 is a promise to read older events, not to accept
fields this reader doesn't know. An additive N still reads N−1 unchanged, and a
removal keeps its old wire type, a cost ADR-0005 already accepts.

Q6 stays open. If `schema_version` becomes a per-line field, the strict structs
must declare it.

### 5. Both schema versions stay at 1

No release exists, no host runs until M5, and no event has been persisted. A v2
would oblige an N−1 reader for a v1 nobody wrote.

Both schemas freeze at the first published release that can parse events; a
stub publish reserving the name doesn't count. Until then, changing v1 takes an
ADR and a `schema:` commit. After it, every wire addition is a bump, because
strict readers refuse it.

## Consequences

**Good**

- A stale or mistyped rate is refused rather than posted
- A platform bug reaches the host as a typed error, not a manual-handling list
- Grouping by country and percentage needs no further event change
- No regulated rate data in the types or the library
- A host that passes extra data, including PII, is told rather than ignored

**Bad**

- `VatRate` is no longer purely a classification. The 2026-10-02 decision now
  covers the Swedish variants only
- After a rate change, an old rate wrongly charged still passes while both are
  listed. A flat table catches typos and long-stale rates, not this
- A zero-rated destination line fails the whole batch until §2 is widened
- The OSS return states "the taxable amount for supplies at standard and reduced
  rate" and "the amount of VAT at standard and reduced rate" (OSS guide, Part 2,
  "Specific details", point 4; footnote 22 adds that more than two rates may be
  needed; **VERIFY, Q10**). A flat list can't say which rate is which. Nothing
  in v0.1 needs it; if an OSS report arrives, the config shape changes and the
  events don't
- Hosts must emit exactly the schema, and they maintain the table

## Alternatives rejected

- **A `charged_percent` field beside `vat_rate`** — either the host invents a
  meaningless class or the field goes optional, and "both" and "neither" become
  runtime checks.
- **A date-effective table** — hand-maintained regulated dates, and a refund
  dated after a change must still carry the old rate. High cost for one narrow
  case.
- **Admitting 0 % in `VatPercent`** — two spellings of zero, and a hole in
  `[vat.rates.se]`.
- **A rate table shipped in the library** — rates change, and a library release
  shouldn't be what corrects a merchant's VAT.
- **Bumping to v2 now** — permanent migration code for data that doesn't exist.
