//! Structural tests for `valid_event_log()`, over its output only.
//!
//! A generator that emits an inconsistent log makes every property test
//! built on it vacuous (docs/05 §2.1), so each consistency rule the
//! generator promises is restated here and checked independently. The
//! payout reference recomputes VAT in `i128` öre with its own rounding,
//! never through `Money::rounded`.
//!
//! The test that `post()` accepts every generated log lands with `post()`
//! in M2. Until then, these are what "valid" means.

use std::collections::{BTreeMap, BTreeSet};

use chrono::Datelike;
use kontera_core::{
    BuyerTaxStatus, CountryCode, Event, EventId, ProviderId, SaleCaptured, SaleLine, SupplyKind,
    VatRate,
};
use kontera_testkit::{default_test_rates, valid_event_log, VatRates, FISCAL_YEAR};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use rust_decimal::Decimal;

/// Reference line VAT in öre: `net × percent / 100`, half away from zero.
/// Net is never negative, so half-away is half-up.
fn reference_vat(net: i128, percent: Decimal) -> i128 {
    let num = net * percent.mantissa();
    let den = 100 * 10i128.pow(percent.scale());
    (2 * num + den) / (2 * den)
}

fn reference_gross(lines: &[SaleLine], rates: &VatRates) -> i128 {
    lines
        .iter()
        .map(|l| {
            let net = l.amount_excl_vat().ore();
            net + reference_vat(net, rates.percent(l.vat_rate))
        })
        .sum()
}

/// Any rate table with percentages 0.00–100.00, at scale 2 like the config.
fn arb_rates() -> impl Strategy<Value = VatRates> {
    let pct = || (0..=10_000i64).prop_map(|bp| Decimal::new(bp, 2));
    (pct(), pct(), pct()).prop_map(|(standard, reduced12, reduced6)| VatRates {
        standard,
        reduced12,
        reduced6,
    })
}

fn by_id(log: &[Event]) -> BTreeMap<&EventId, &Event> {
    log.iter().map(|e| (e.id(), e)).collect()
}

fn sale<'a>(log: &BTreeMap<&EventId, &'a Event>, id: &EventId) -> Option<&'a SaleCaptured> {
    match log.get(id) {
        Some(Event::SaleCaptured(s)) => Some(s),
        Some(Event::RefundIssued(_) | Event::FeeCharged(_) | Event::PayoutSettled(_)) | None => {
            None
        }
    }
}

/// The provider an event belongs to; a refund's is its sale's.
fn provider_of<'a>(log: &BTreeMap<&EventId, &'a Event>, e: &'a Event) -> Option<&'a ProviderId> {
    match e {
        Event::SaleCaptured(s) => Some(&s.provider),
        Event::RefundIssued(r) => sale(log, &r.original_sale).map(|s| &s.provider),
        Event::FeeCharged(f) => Some(&f.provider),
        Event::PayoutSettled(p) => Some(&p.provider),
    }
}

/// Net per `(rate, kind, description)`, in öre. Order-free.
fn net_by_line(lines: &[SaleLine]) -> BTreeMap<(VatRate, SupplyKind, &str), i128> {
    let mut m = BTreeMap::new();
    for l in lines {
        *m.entry((l.vat_rate, l.goods_or_services, l.description.as_str()))
            .or_insert(0) += l.amount_excl_vat().ore();
    }
    m
}

proptest! {
    /// I7: no id appears twice.
    #[test]
    fn event_ids_are_unique(log in valid_event_log(default_test_rates())) {
        let ids: BTreeSet<_> = log.iter().map(Event::id).collect();
        prop_assert_eq!(ids.len(), log.len());
    }

    /// Every date falls in the fiscal year, so nothing is `OutsidePeriod`.
    #[test]
    fn every_date_is_inside_the_fiscal_year(log in valid_event_log(default_test_rates())) {
        for e in &log {
            prop_assert_eq!(e.at().year(), FISCAL_YEAR, "{}", e.id());
        }
    }

    /// Sales are `SeDomestic` at a non-zero rate, and no sale or refund is
    /// empty — the M2 `validate` stage would reject one.
    #[test]
    fn sales_are_domestic_and_lines_are_never_empty(
        log in valid_event_log(default_test_rates()),
    ) {
        for e in &log {
            match e {
                Event::SaleCaptured(s) => {
                    prop_assert_eq!(s.ship_to, CountryCode::SE);
                    prop_assert!(!s.lines.is_empty());
                    prop_assert!(s.lines.iter().all(|l| l.vat_rate != VatRate::Zero));
                }
                Event::RefundIssued(r) => prop_assert!(!r.lines.is_empty()),
                Event::FeeCharged(_) | Event::PayoutSettled(_) => {}
            }
        }
    }

    /// A refund names a sale in the log, is not dated before it, and —
    /// summed over every refund of that sale — never returns more of any
    /// line than was sold.
    #[test]
    fn refunds_reference_a_prior_sale_and_never_exceed_it(
        log in valid_event_log(default_test_rates()),
    ) {
        let index = by_id(&log);
        let mut refunded: BTreeMap<&EventId, Vec<SaleLine>> = BTreeMap::new();
        for e in &log {
            if let Event::RefundIssued(r) = e {
                let original = sale(&index, &r.original_sale);
                prop_assert!(original.is_some(), "{} → {}", r.id, r.original_sale);
                prop_assert!(original.is_some_and(|s| s.at <= r.at), "{}", r.id);
                refunded.entry(&r.original_sale).or_default().extend(r.lines.iter().cloned());
            }
        }
        for (id, lines) in refunded {
            let sold = sale(&index, id).map(|s| net_by_line(&s.lines)).unwrap_or_default();
            for (key, amount) in net_by_line(&lines) {
                prop_assert!(amount <= sold.get(&key).copied().unwrap_or(0), "{} {:?}", id, key);
            }
        }
    }

    /// Every payout covers real, non-payout events of its own provider,
    /// none dated after it; every other event is covered exactly once.
    #[test]
    fn every_event_is_covered_once_by_a_later_payout_of_its_provider(
        log in valid_event_log(default_test_rates()),
    ) {
        let index = by_id(&log);
        let mut covered: BTreeMap<&EventId, usize> = BTreeMap::new();
        for e in &log {
            let Event::PayoutSettled(p) = e else { continue };
            for id in &p.covers {
                let target = index.get(id).copied();
                prop_assert!(target.is_some(), "{} covers missing {}", p.id, id);
                let Some(target) = target else { continue };
                prop_assert!(!matches!(target, Event::PayoutSettled(_)), "{}", id);
                prop_assert_eq!(provider_of(&index, target), Some(&p.provider), "{}", id);
                prop_assert!(target.at() <= p.at, "{} after {}", id, p.id);
                *covered.entry(id).or_insert(0) += 1;
            }
        }
        for e in &log {
            if !matches!(e, Event::PayoutSettled(_)) {
                prop_assert_eq!(covered.get(e.id()).copied(), Some(1), "{}", e.id());
            }
        }
    }

    /// I2 by independent reference: each payout equals Σ gross sales −
    /// Σ gross refunds − Σ fees over what it covers, in `i128` öre — at any
    /// rate table, so the generator honours its parameter rather than a
    /// number of its own.
    #[test]
    fn payout_amount_reconciles_by_independent_reference(
        (rates, log) in arb_rates().prop_flat_map(|r| (Just(r), valid_event_log(r))),
    ) {
        let index = by_id(&log);
        for e in &log {
            let Event::PayoutSettled(p) = e else { continue };
            let expected: i128 = p.covers.iter().map(|id| match index.get(id) {
                Some(Event::SaleCaptured(s)) => reference_gross(&s.lines, &rates),
                Some(Event::RefundIssued(r)) => -reference_gross(&r.lines, &rates),
                Some(Event::FeeCharged(f)) => -f.amount().ore(),
                Some(Event::PayoutSettled(_)) | None => 0,
            }).sum();
            prop_assert_eq!(p.amount.ore(), expected, "{}", p.id);
        }
    }
}

/// The reference rounding agrees with hand-worked cases, docs/01 §1.1 first.
#[test]
fn reference_vat_matches_hand_worked_cases() {
    let pct = |s: &str| s.parse::<Decimal>().unwrap();
    assert_eq!(reference_vat(4_000_000, pct("25.00")), 1_000_000);
    assert_eq!(reference_vat(2, pct("25.00")), 1); // 0.5 öre, up
    assert_eq!(reference_vat(1, pct("6.00")), 0); // 0.06 öre, down
    assert_eq!(reference_vat(25, pct("6")), 2); // 1.5 öre, up
}

/// Non-vacuity: across a fixed sample, every feature the generator
/// promises actually occurs. A generator that silently stopped emitting
/// refunds would pass every property above.
#[test]
fn the_generator_exercises_every_feature() {
    let strategy = valid_event_log(default_test_rates());
    let mut runner = TestRunner::deterministic();
    let mut seen = BTreeSet::new();
    for _ in 0..256 {
        let log = strategy.new_tree(&mut runner).unwrap().current();
        let index = by_id(&log);
        if log
            .iter()
            .filter(|e| matches!(e, Event::PayoutSettled(_)))
            .count()
            > 1
        {
            seen.insert("several payouts".to_owned());
        }
        for e in &log {
            match e {
                Event::SaleCaptured(s) => {
                    seen.insert(format!("provider {}", s.provider));
                    if matches!(s.buyer, BuyerTaxStatus::Business { .. }) {
                        seen.insert("business buyer".to_owned());
                    }
                    for l in &s.lines {
                        seen.insert(format!("rate {:?}", l.vat_rate));
                        seen.insert(format!("supply {:?}", l.goods_or_services));
                        if !l.description.is_ascii() {
                            seen.insert("non-ASCII description".to_owned());
                        }
                    }
                }
                Event::RefundIssued(r) => {
                    seen.insert("refund".to_owned());
                    let sold = sale(&index, &r.original_sale).map(|s| net_by_line(&s.lines));
                    let partial = net_by_line(&r.lines)
                        .iter()
                        .any(|(k, v)| sold.as_ref().and_then(|m| m.get(k)).is_some_and(|s| v < s));
                    if partial {
                        seen.insert("partial refund".to_owned());
                    }
                }
                Event::FeeCharged(f) => {
                    seen.insert(format!("fee {:?}", f.kind));
                }
                Event::PayoutSettled(_) => {}
            }
        }
    }
    let expected = [
        "several payouts",
        "provider stripe",
        "provider klarna",
        "business buyer",
        "rate Standard",
        "rate Reduced12",
        "rate Reduced6",
        "supply Goods",
        "supply Services",
        "non-ASCII description",
        "refund",
        "partial refund",
        "fee Transaction",
        "fee Payout",
        "fee Dispute",
        "fee Other",
    ];
    for feature in expected {
        assert!(seen.contains(feature), "never generated: {feature}");
    }
}
