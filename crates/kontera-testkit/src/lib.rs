//! Property-test generators shared across Kontera's test crates.
//!
//! This crate exists because each file under `tests/` compiles as its own
//! crate, and `kontera-sie`'s golden tests need generated ledgers across a
//! crate boundary. It is never published. See `docs/03-architecture.md` §3.1.
//!
//! Use it from integration tests (`tests/`) only. An in-module
//! `#[cfg(test)]` block inside `kontera-core` that imports this crate sees
//! two `kontera_core`s — the one being compiled and the one linked here —
//! and their types do not unify.
//!
//! [`valid_event_log`] is itself tested (`tests/valid_event_log.rs`): a
//! generator that emits a log `post()` rejects is a generator bug
//! (docs/05 §2.1).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use chrono::NaiveDate;
use kontera_core::{
    BuyerTaxStatus, CountryCode, Event, EventId, FeeCharged, FeeKind, Money, PayoutSettled,
    ProviderId, RefundIssued, SaleCaptured, SaleLine, SupplyKind, VatNumber, VatRate,
};
use proptest::prelude::*;
use rust_decimal::Decimal;

/// The calendar year every generated event falls in. Matches the
/// `[fiscal_year]` of the config example (docs/04 §3).
pub const FISCAL_YEAR: i32 = 2026;

/// Provider ids, the `[accounts.receivable]` keys of the config example.
const PROVIDERS: [&str; 2] = ["stripe", "klarna"];

/// Line descriptions. Two of them need CP437's `å ä ö` (risk R2), so a SIE
/// golden test built on this generator exercises the encoder for free.
const DESCRIPTIONS: [&str; 4] = ["Widget", "Kaffekopp", "Blåbärssylt", "Ölglas"];

/// A fictional VAT number for business buyers. Shape only: VIES is the
/// host's job (docs/04 §2.1).
const BUSINESS_VAT_NUMBER: &str = "SE000000000001";

/// Per-line net ceiling, 10 000 kr. Keeps every derived sum far inside
/// `i64` öre, so no arithmetic below can overflow.
const MAX_LINE_ORE: i64 = 1_000_000;

/// Output VAT percentages per rate class: the shape of `[vat.rates]` in the
/// config schema (docs/04 §3).
///
/// A parameter, not a constant, so the generator never hardcodes a regulated
/// number. Superseded by the real rate table when `Config` lands in M2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VatRates {
    /// `se_standard`, in percent.
    pub standard: Decimal,
    /// `se_reduced12`, in percent.
    pub reduced12: Decimal,
    /// `se_reduced6`, in percent.
    pub reduced6: Decimal,
}

impl VatRates {
    /// The percentage for a rate class. `Zero` is zero by definition.
    #[must_use]
    pub fn percent(&self, rate: VatRate) -> Decimal {
        match rate {
            VatRate::Standard => self.standard,
            VatRate::Reduced12 => self.reduced12,
            VatRate::Reduced6 => self.reduced6,
            VatRate::Zero => Decimal::ZERO,
        }
    }
}

/// The rates of the config example (docs/04 §3). For tests only.
// VERIFY: 25 / 12 / 6 % are the values in docs/04 §3. Confirm against
// Mervärdesskattelagen (2023:200) and current Skatteverket guidance before
// any of them leave test code — in particular which goods sit in which
// reduced class, which has moved before and may move again.
#[must_use]
pub fn default_test_rates() -> VatRates {
    VatRates {
        standard: Decimal::new(2500, 2),
        reduced12: Decimal::new(1200, 2),
        reduced6: Decimal::new(600, 2),
    }
}

/// An internally consistent event log, every event settled.
///
/// One to four batches. Each batch belongs to one provider and holds one to
/// five domestic sales, at most one partial refund per sale, zero to three
/// fees, and one payout that covers all of them. The payout amount is
/// `Σ gross sales − Σ gross refunds − Σ fees` (I2), with VAT computed per
/// line at `rates` and rounded half away from zero (ADR-0002). Ids are
/// unique (I7); every date falls in [`FISCAL_YEAR`]; a refund never
/// precedes its sale, and a payout never precedes what it covers.
///
/// Scope, until M2's `vat.rs` decides how each scenario is posted: only
/// `SeDomestic` sales (ship-to `SE`, consumer or business buyer) at the
/// three non-zero rates, and no unsettled events. Cross-border scenarios
/// and an unsettled tail are added here alongside the rules that post them.
///
/// Events are in generation order. Order carries no meaning (I6).
pub fn valid_event_log(rates: VatRates) -> impl Strategy<Value = Vec<Event>> {
    prop::collection::vec(arb_batch(), 1..=4).prop_map(move |batches| build(&batches, rates))
}

/// One sale line before ids and dates are assigned.
#[derive(Clone, Debug)]
struct LinePlan {
    net: i64,
    rate: VatRate,
    kind: SupplyKind,
    description: usize,
    /// Share of this line to refund, in thousandths, if any.
    refund_permille: Option<i64>,
}

#[derive(Clone, Debug)]
struct SalePlan {
    day: u32,
    business: bool,
    lines: Vec<LinePlan>,
    refund_lag: u32,
}

#[derive(Clone, Debug)]
struct BatchPlan {
    provider: usize,
    /// Days after 1 January. With every offset below added, the latest
    /// event is 350 days in — 17 December, inside the year.
    base: u32,
    sales: Vec<SalePlan>,
    fees: Vec<(u32, FeeKind, i64)>,
    payout_lag: u32,
}

fn arb_line() -> impl Strategy<Value = LinePlan> {
    let rate = prop_oneof![
        Just(VatRate::Standard),
        Just(VatRate::Reduced12),
        Just(VatRate::Reduced6),
    ];
    let kind = prop_oneof![Just(SupplyKind::Goods), Just(SupplyKind::Services)];
    (
        0..=MAX_LINE_ORE,
        rate,
        kind,
        0..DESCRIPTIONS.len(),
        prop::option::of(1..=1000i64),
    )
        .prop_map(|(net, rate, kind, description, refund_permille)| LinePlan {
            net,
            rate,
            kind,
            description,
            refund_permille,
        })
}

fn arb_sale() -> impl Strategy<Value = SalePlan> {
    (
        0..=7u32,
        any::<bool>(),
        prop::collection::vec(arb_line(), 1..=4),
        0..=7u32,
    )
        .prop_map(|(day, business, lines, refund_lag)| SalePlan {
            day,
            business,
            lines,
            refund_lag,
        })
}

fn arb_batch() -> impl Strategy<Value = BatchPlan> {
    let fee_kind = prop_oneof![
        Just(FeeKind::Transaction),
        Just(FeeKind::Payout),
        Just(FeeKind::Dispute),
        Just(FeeKind::Other),
    ];
    (
        0..PROVIDERS.len(),
        0..=330u32,
        prop::collection::vec(arb_sale(), 1..=5),
        prop::collection::vec((0..=14u32, fee_kind, 1..=50_000i64), 0..=3),
        0..=5u32,
    )
        .prop_map(|(provider, base, sales, fees, payout_lag)| BatchPlan {
            provider,
            base,
            sales,
            fees,
            payout_lag,
        })
}

fn build(batches: &[BatchPlan], rates: VatRates) -> Vec<Event> {
    let mut log = Vec::new();
    for (b, batch) in batches.iter().enumerate() {
        let provider = ProviderId::parse(PROVIDERS[batch.provider]).expect("listed ids are valid");
        let mut covers = Vec::new();
        let mut payout = Money::ZERO;

        for (s, sale) in batch.sales.iter().enumerate() {
            let id = event_id(&format!("ord_{b}_{s}"));
            let at = date(batch.base + sale.day);
            let lines: Vec<SaleLine> = sale.lines.iter().map(|l| sale_line(l, l.net)).collect();
            payout = payout + lines.iter().map(|l| gross(l, rates)).sum::<Money>();
            let buyer = if sale.business {
                BuyerTaxStatus::Business {
                    vat_number: VatNumber::parse(BUSINESS_VAT_NUMBER).expect("non-empty"),
                }
            } else {
                BuyerTaxStatus::Consumer
            };
            covers.push(id.clone());
            log.push(Event::SaleCaptured(SaleCaptured {
                id: id.clone(),
                at,
                provider: provider.clone(),
                buyer,
                ship_to: CountryCode::SE,
                lines,
            }));

            // A refunded share of 1–1000 ‰, at least one öre, is never more
            // than the line: net × p / 1000 ≤ net, and net ≥ 1 here.
            let refund: Vec<SaleLine> = sale
                .lines
                .iter()
                .filter(|l| l.net > 0)
                .filter_map(|l| {
                    let p = l.refund_permille?;
                    Some(sale_line(l, (l.net * p / 1000).max(1)))
                })
                .collect();
            if !refund.is_empty() {
                let refund_id = event_id(&format!("ref_{b}_{s}"));
                payout = payout - refund.iter().map(|l| gross(l, rates)).sum::<Money>();
                covers.push(refund_id.clone());
                log.push(Event::RefundIssued(RefundIssued {
                    id: refund_id,
                    at: date(batch.base + sale.day + sale.refund_lag),
                    original_sale: id,
                    lines: refund,
                }));
            }
        }

        for (f, &(day, kind, ore)) in batch.fees.iter().enumerate() {
            let id = event_id(&format!("fee_{b}_{f}"));
            let fee = FeeCharged::new(
                id.clone(),
                date(batch.base + day),
                provider.clone(),
                kind,
                Money::from_ore(ore),
            )
            .expect("generated fees are at least one öre");
            payout = payout - fee.amount();
            covers.push(id);
            log.push(Event::FeeCharged(fee));
        }

        // Sales land by day 7, refunds by 14, fees by 14: day 15 is after all.
        log.push(Event::PayoutSettled(PayoutSettled {
            id: event_id(&format!("po_{b}")),
            at: date(batch.base + 15 + batch.payout_lag),
            provider,
            amount: payout,
            covers,
        }));
    }
    log
}

fn sale_line(plan: &LinePlan, ore: i64) -> SaleLine {
    SaleLine::new(
        DESCRIPTIONS[plan.description].to_owned(),
        Money::from_ore(ore),
        plan.rate,
        plan.kind,
    )
    .expect("generated amounts are never negative")
}

/// Line VAT: net × percent / 100, rounded half away from zero, per line.
fn vat(line: &SaleLine, rates: VatRates) -> Money {
    let net = Decimal::from(line.amount_excl_vat());
    Money::rounded(net * rates.percent(line.vat_rate) / Decimal::ONE_HUNDRED)
        .expect("bounded by MAX_LINE_ORE")
}

fn gross(line: &SaleLine, rates: VatRates) -> Money {
    line.amount_excl_vat() + vat(line, rates)
}

fn event_id(s: &str) -> EventId {
    EventId::parse(s).expect("generated ids are non-empty")
}

/// `days` after 1 January of [`FISCAL_YEAR`].
fn date(days: u32) -> NaiveDate {
    NaiveDate::from_yo_opt(FISCAL_YEAR, days + 1).expect("offsets stay below day 365")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ore: i64, rate: VatRate) -> SaleLine {
        SaleLine::new(
            "x".to_owned(),
            Money::from_ore(ore),
            rate,
            SupplyKind::Goods,
        )
        .unwrap()
    }

    /// docs/01 §1.1: 40 000 kr net at the standard rate carries 10 000 kr.
    #[test]
    fn worked_example_vat_matches_the_posting_table() {
        let l = line(4_000_000, VatRate::Standard);
        assert_eq!(vat(&l, default_test_rates()), Money::from_ore(1_000_000));
        assert_eq!(gross(&l, default_test_rates()), Money::from_ore(5_000_000));
    }

    /// 2 öre × 25 % = 0.5 öre rounds up; 1 öre × 6 % = 0.06 öre rounds down.
    #[test]
    fn line_vat_rounds_half_away_from_zero() {
        let rates = default_test_rates();
        assert_eq!(vat(&line(2, VatRate::Standard), rates), Money::from_ore(1));
        assert_eq!(vat(&line(1, VatRate::Reduced6), rates), Money::ZERO);
        assert_eq!(vat(&line(1, VatRate::Zero), rates), Money::ZERO);
    }

    /// The latest offset the plans can produce is still inside the year.
    #[test]
    fn the_latest_generated_day_is_inside_the_fiscal_year() {
        assert_eq!(
            date(330 + 15 + 5),
            NaiveDate::from_ymd_opt(FISCAL_YEAR, 12, 17).unwrap()
        );
    }
}
