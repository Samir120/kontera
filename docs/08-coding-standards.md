# 08 — Coding Standards

**Status:** draft · **Owner:** Samir · **Last reviewed:** 2026-09-03

Rust conventions for this codebase. Where general "clean code" advice conflicts
with idiomatic Rust, this document wins — much of that advice was written for
languages with different constraints, and following it here produces worse code.

---

## 1. The governing principle

**Make illegal states unrepresentable.**

This isn't a style preference; it's the project's reason for existing in Rust. A
validation function that checks a verification balances can be forgotten at a
call site. A `BalancedTransaction` whose constructor is the only way to build one
cannot.

Before writing a check, ask whether the type system can make the check
unnecessary.

```rust
// no
fn validate(v: &Verification) -> Result<(), Error> { … }

// yes
impl BalancedTransaction {
    pub fn new(lines: Vec<Line>) -> Result<Self, PostingError> {
        let sum: Money = lines.iter().map(|l| l.amount).sum();
        if sum != Money::ZERO {
            return Err(PostingError::Unbalanced { … });
        }
        Ok(Self { lines })   // field is private
    }
}
```

## 2. Parse, don't validate

Boundaries turn untrusted input into proven types. Once past the boundary,
nothing re-checks.

```rust
pub struct AccountNumber(u16);          // private field

impl AccountNumber {
    pub fn parse(s: &str) -> Result<Self, ParseError> { … }   // 1000–8999
    pub fn class(&self) -> AccountKind { … }                  // infallible
}
```

`class()` returns `AccountKind`, not `Result<AccountKind>`, because by the time
you hold an `AccountNumber` the range is already proven.

## 3. Newtypes over primitives

No bare `String`, `i64` or `f64` for a domain concept. Ever.

| Concept | Type | Not |
|---|---|---|
| Money | `Money` | `f64`, `Decimal` |
| Account | `AccountNumber` | `u16`, `String` |
| Event identity | `EventId` | `String` |
| Country | `CountryCode` | `String` |
| VAT rate | `VatRate` (enum) | `Decimal` |

The cost is a few lines of boilerplate. The benefit is that passing an
`AccountNumber` where an `EventId` is expected doesn't compile.

## 4. Errors

- `thiserror` in library crates. `anyhow` only in `kontera-cli`, never in core.
- One error enum per crate; nested enums for sub-domains (`ScenarioGap`).
- **Error variants carry data, not prose.** `SettlementMismatch { delta }` is
  useful; `SettlementMismatch(String)` is not. The host needs to branch on it.
- Every error message answers: what was wrong, which event, what to do next.
- **No `unwrap()` or `expect()` in library code.** `kontera-core` sets
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`. If an
  invariant genuinely guarantees safety, use `expect("…")` with a comment
  explaining the proof — and expect to defend it in review.
- `post()` never panics on any input. This is fuzz-tested. A panic in WASM takes
  down the host's request.

## 5. Module structure

Organise by **domain concept**, not by technical layer.

```
src/
  money.rs          ✅  a concept
  verification.rs   ✅
  vat.rs            ✅
  settle.rs         ✅

  models.rs         ❌  a layer
  services.rs       ❌
  utils.rs          ❌  a place where code goes to hide
  helpers.rs        ❌
```

If something doesn't belong to a domain concept, that's a signal the concept is
missing, not that a `utils` module is needed.

## 6. Exhaustive matching is a compliance mechanism

**No `_ =>` wildcard arms on domain enums.** That means `VatScenario`, `Event`,
`ScenarioGap`, `PostingError`, `VatRate`, `SupplyKind`, `AccountKind`.

The argument for writing this project in Rust rests substantially on this. When
a new VAT scenario is added to the enum, the compiler lists every place that must
now handle it. A wildcard arm converts that build failure into silent, plausible,
wrong behaviour — which in this domain means a wrong momsdeklaration.

```rust
// NO — the day EuB2cOss is added, this compiles and books it as domestic
match scenario {
    VatScenario::SeDomestic => post_domestic(line, cfg),
    _ => post_domestic(line, cfg),
}

// YES — adding a variant breaks the build, which is the point
match scenario {
    VatScenario::SeDomestic      => post_domestic(line, cfg),
    VatScenario::EuB2bReverse    => post_reverse_charge(line, cfg),
    VatScenario::EuB2cOss        => post_oss(line, cfg),
    VatScenario::ExportOutsideEu => post_export(line, cfg),
}
```

A wildcard is acceptable only on enums the project does not own (`std::io::Error`
kinds, third-party enums marked `#[non_exhaustive]`) where exhaustiveness is
impossible anyway.

Enforce with `#![deny(clippy::wildcard_enum_match_arm)]` in `kontera-core`.

## 7. Naming

**Swedish where it is the domain's real name, English everywhere else.**

Swedish: `Verifikation`, `moms`, `konto` when referring to the accounting
concept. English: everything structural — `Ledger`, `post`, `Config`, `Event`.

The rule is: if an accountant would say the Swedish word and there's no clean
English equivalent, keep Swedish. `utgående moms` is not "outgoing VAT" to anyone
who does this work. But `Verification` reads fine in English and is used
throughout the docs, so it stays English.

Document any new Swedish identifier in the glossary in
[02 §1](02-domain-model.md#1-glossary). If it isn't in the glossary, it isn't a
domain term, it's jargon.

## 8. What *not* to do — OOP reflexes that hurt here

| Pattern | Why it's wrong here |
|---|---|
| Repository pattern | There is no persistence. ADR-0001 |
| Dependency injection container | Pass arguments to functions |
| Service layer | The functions *are* the service |
| Trait "for testability" | Pure functions are already testable. A trait with one impl is indirection with no payoff |
| `Arc<Mutex<_>>` | No concurrency in this codebase |
| Interface for every struct | Rust traits are for shared behaviour, not for ceremony |
| Extracting every 5-line block | A long, flat `match` is often the clearest expression of a rule. Helper soup hides the logic |
| Builder for a 3-field struct | Struct literal |

**Rule of three.** Don't abstract until the third occurrence. Two similar things
are frequently a coincidence, and premature abstraction over accounting rules is
particularly costly — VAT scenarios that look alike often diverge on the next
regulation change.

## 9. Functions

- One thing, but "one thing" is domain-sized, not line-count-sized. A function
  that determines a VAT scenario is one thing even if it's forty lines of `match`.
- Prefer returning data over mutating arguments.
- `&[T]` in parameters, not `&Vec<T>`.
- Avoid `impl Trait` in public return position — it constrains what can change
  later without a semver break.

## 10. Determinism rules

These are correctness requirements, not style ([ADR-0001](adr/0001-pure-core.md),
[03 §4.2](03-architecture.md#42-determinism-rules)):

- `BTreeMap` / `BTreeSet` in anything reaching output. Never `HashMap`.
- Sort explicitly by `(date, event_id)` before numbering. Never rely on input
  order.
- No `SystemTime::now()`, `Utc::now()` or `Local::now()` in `core` or `sie`.
  Dates are parameters.
- No randomness outside test generators.

CI enforces these via `scripts/check-purity.sh`.

## 11. Documentation

- `#![warn(missing_docs)]` on public items in `kontera-core`.
- Doc comments explain **why and when**, not what the signature already says.
- Every fallible public function has an `# Errors` section listing the variants
  it can return and what causes each.
- Doc examples compile and run (`cargo test --doc`). An example that doesn't
  compile is worse than none.
- Regulatory logic carries a source reference in a comment:

```rust
// Reverse charge for EU B2B. Buyer accounts for VAT in their member state.
// See docs/02-domain-model.md §3 (EuB2bReverse). VERIFY against ML (2023:200).
```

## 12. Tests

- Unit tests: `#[cfg(test)] mod tests` at the bottom of the module they test.
- Integration and property tests: `tests/`.
- Test names are sentences: `refund_reverses_original_vat_rate`, not `test_refund_2`.
- **For anything touching an invariant, the property test is written first.** Not
  TDD dogma — invariants are the product, so they're the specification.
- Golden files are regenerated only with `KONTERA_BLESS=1`, and every change is
  reviewed as a diff.

## 13. Dependencies

`kontera-core` may depend on: `rust_decimal`, `serde`, a date crate, `thiserror`.

Anything else needs a justification in the PR. Every dependency is code a future
auditor has to read and a supply-chain surface this project doesn't control.
When in doubt, write the twenty lines.

Dev-dependencies (`proptest`, `serde_json`, `kontera-testkit`) are not subject
to this list; they never ship. They still need a reason.

## 14. Formatting and lints

Default `rustfmt`. No custom `rustfmt.toml` — arguing about formatting is time
not spent on the domain.

Lint configuration in `kontera-core/src/lib.rs`:

```rust
#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
```

`clippy::pedantic` will produce noise. Fix it or `#[allow]` it with a reason
comment. Don't disable it wholesale.
