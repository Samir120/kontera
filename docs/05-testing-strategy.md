# 05 — Testing Strategy

**Status:** draft · **Owner:** Samir · **Last reviewed:** 2026-09-03

The core is a pure function, which means it can be tested in ways most business
software cannot. That is the main reason the architecture looks the way it does.

---

## 1. Test pyramid

| Layer | Tool | What it proves | Runs |
|---|---|---|---|
| Unit | `#[test]` | Individual rules, edge cases, error messages | every commit |
| Property | `proptest` | Invariants hold for *all* inputs | every commit |
| Golden | fixtures + byte compare | Output is exactly this, forever | every commit |
| Fuzz | `cargo-fuzz` | No panics on arbitrary input | nightly |
| Acceptance | manual, recorded | A commercial system imports the file | per milestone |

## 2. Property tests — the centrepiece

Each maps to an invariant from [02 §6](02-domain-model.md#6-invariants).

```rust
proptest! {
    /// I1 — every verification balances, for any event sequence
    #[test]
    fn all_verifications_balance(events in valid_event_log()) {
        let ledger = post(&events, &default_config()).unwrap();
        for v in &ledger.verifications {
            prop_assert_eq!(v.lines.iter().map(|l| l.amount).sum::<Money>(), Money::ZERO);
        }
    }

    /// I6 — output is independent of input ordering
    #[test]
    fn order_independent(events in valid_event_log(), seed in any::<u64>()) {
        let a = post(&events, &default_config()).unwrap();
        let b = post(&shuffle(&events, seed), &default_config()).unwrap();
        prop_assert_eq!(write_4i(&a, FIXED_DATE)?, write_4i(&b, FIXED_DATE)?);
    }

    /// I8 — VAT report reconciles to the postings it summarises
    #[test]
    fn vat_report_reconciles(events in valid_event_log()) {
        let l = post(&events, &default_config()).unwrap();
        for (rate, reported) in l.vat_report.output_vat_by_rate() {
            let posted = l.sum_of_account(config.vat_account(rate));
            prop_assert_eq!(reported, -posted);   // liability is a credit
        }
    }

    /// I2 — a broken settlement is ALWAYS an error, never a plausible answer
    #[test]
    fn broken_settlement_never_succeeds(
        events in valid_event_log(),
        delta in nonzero_money(),
    ) {
        let tampered = perturb_payout_amount(events, delta);
        prop_assert!(matches!(
            post(&tampered, &default_config()),
            Err(PostingError::SettlementMismatch { .. })
        ));
    }
}
```

### 2.1 The generator is the hard part

`valid_event_log()` must produce *internally consistent* logs: sales before
refunds, refunds not exceeding originals, payouts whose `covers` reference real
events and whose amounts actually reconcile. Writing that generator is a day of
work and it is the highest-leverage day in the project — it is what makes every
property test above meaningful rather than vacuous.

**Rule:** the generator lives in the `kontera-testkit` crate
([03 §3.1](03-architecture.md#31-why-kontera-testkit-is-a-crate-not-a-test-module))
and is itself tested.
A generator that emits a log `post()` rejects is a generator bug, and there
should be a test asserting `valid_event_log()` output always yields `Ok`.

### 2.2 Rounding

Rounding deserves its own property: for a mixed-VAT order, the sum of per-line
VAT computed at line level must equal the VAT posted, with no cent drift. Generate
many small lines at 6% specifically — that is where half-up vs banker's rounding
diverges most visibly.

## 3. Golden files

```
fixtures/
├── basic-settlement/        # the worked example from 01 §1.1
│   ├── events.jsonl
│   ├── config.toml
│   └── expected.si          # byte-compared
├── partial-refund/
├── mixed-vat-rates/
├── eu-b2b-reverse/
├── export-outside-eu/
├── swedish-characters/      # å ä ö in company name and descriptions — CP437
└── unsupported-scenario/    # asserts the ERROR, not the output
```

Golden files catch what property tests can't: accidental format changes, header
drift, encoding regressions. Byte comparison, not parse-and-compare — the whole
point is that the bytes are the contract.

**Regenerating goldens requires an explicit env var** (`KONTERA_BLESS=1`) so
nobody silently blesses a regression. Every golden change is reviewed as a diff.

### 3.1 Error goldens

Fixtures that assert failure are as valuable as ones that assert success. If
`unsupported-scenario/` ever starts returning `Ok`, the fail-closed guarantee has
broken. Snapshot the error message too — error text is part of the UX.

## 4. Fuzzing

`cargo-fuzz` targets:

1. `post()` with arbitrary deserialised events — must never panic, only `Ok` or
   `Err`
2. The JSON deserialiser — malformed input must produce a clean error
3. The CP437 encoder — arbitrary Unicode in, either valid CP437 bytes or a clean
   `Err(UnrepresentableCharacter)`, never a panic and never mojibake

Nightly in CI, corpus committed.

## 5. The acceptance test

> **A generated `.si` file imports into a Fortnox or Visma trial account with
> zero manual correction.**

This is the single highest-risk item in the project and the cheapest to check.
**Do it in week 6, not week 10.** If a commercial system rejects the output,
nothing else built matters, and the remaining four weeks are the wrong four
weeks.

Procedure, recorded in `docs/acceptance/`:

1. Generate `fixtures/basic-settlement/expected.si`
2. Import into a Fortnox trial account
3. Screenshot the resulting verification list
4. Compare each posting to the table in [01 §1.1](01-problem-and-scope.md)
5. Repeat with `swedish-characters/` — this is the one that will fail first
6. Record result, date, and target system version

Repeat for every SIE writer change. Encoding and header handling are exactly the
kind of thing that regresses silently.

## 6. Mechanical guarantees in CI

Enforced by the build, not by discipline:

| Guarantee | How |
|---|---|
| No `unsafe` in core | `#![forbid(unsafe_code)]` in `lib.rs` |
| No async in core | Dependency check — no `tokio`/`futures` in core's tree |
| No I/O in core | `cargo-deny` ban list + review; no `std::fs`/`std::net` imports |
| No clock reads | Grep for `SystemTime::now` / `Utc::now` in `core` and `sie` |
| No `HashMap` in output paths | Clippy lint + review |
| Lints clean | `cargo clippy --all-targets -- -D warnings` |
| Formatted | `cargo fmt --check` |
| Dependencies audited | `cargo audit`, `cargo deny check` |
| MSRV honoured | Build matrix includes the pinned MSRV |

## 7. Domain review — the non-mechanical part

No amount of `proptest` proves the *accounting* is right. It proves the code does
what the code says.

Two reviews required before any public release:

1. **Chart mapping review** — a Swedish accountant confirms the default BAS
   account table in [02 §4](02-domain-model.md#4-default-account-mapping).
2. **VAT scenario review** — the same person confirms the four scenarios and,
   more importantly, that the *excluded* ones fail rather than being mishandled.

Record both in `docs/reviews/` with date and reviewer. Every **VERIFY** marker
in the docs must be resolved or explicitly deferred by then.

## 8. Coverage

Tracked, not targeted. `cargo-llvm-cov` in CI, reported, no threshold gate.
`kontera-core` should approach full coverage naturally because it is pure; if a
branch is uncovered, that is a signal about the branch, not a number to chase.

## 9. What is not tested

Per [01 §5.3](01-problem-and-scope.md#53-explicit-anti-goals-for-v01): no
performance tests, no load tests, no concurrency tests. This is a fold over a few
thousand events that runs once a day. Adding benchmarks would be optimising the
wrong thing and would imply a promise the project isn't making.
