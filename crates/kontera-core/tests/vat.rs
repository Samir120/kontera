//! Tests for `VatPercent`, `line_vat`, the territory tables, `classify` and
//! `check_sale`, over the public API only.
//!
//! `VatPercent`'s grammar is restated here as an independent predicate, and
//! the parser must agree with it on every string of up to five characters
//! drawn from `0-9` and `.` — 177 156 strings, which covers every accepted
//! value and every way to misplace a digit or a point. Characters outside
//! that alphabet are spot-checked.
//!
//! `line_vat` is checked against an `i128` reference with its own rounding.
//! The rate table in these tests is deliberately not Sweden's, so a number
//! hardcoded in the code would fail them.
//!
//! The territory tables are checked over all 676 country codes. The VAT area
//! is compared with a list restated here. The ISO list is pinned by its size
//! and by how many codes start and end with each letter, which catches a
//! dropped, repeated or mistyped entry without a second copy of 249 codes.
//!
//! `classify` is checked on every code with both kinds of buyer, 1 352 sales,
//! against the docs/02 §3 conditions restated here. How many codes land in
//! each scenario is pinned too, so the restatement can't drift with the code.
//!
//! `check_sale` is checked on each of those sales with one line, in every
//! form a line's rate and kind can take: 16 224 sales against the ADR-0006 §1
//! table restated here. How many give each outcome is pinned as well. Every
//! sale of up to three lines, to nine codes, then proves that the first failing
//! line decides the outcome.

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use kontera_core::{
    check_sale, classify, in_eu_vat_area, is_assigned_country_code, line_vat, BuyerTaxStatus,
    CountryCode, EventId, Money, PostingError, ProviderId, SaleCaptured, SaleLine, ScenarioGap,
    SeRates, SupplyKind, VatNumber, VatPercent, VatRate, VatRates, VatScenario,
};
use proptest::prelude::*;
use rust_decimal::Decimal;

/// The `VatPercent` grammar, restated: the value in hundredths if accepted.
fn reference_percent(s: &str) -> Option<u32> {
    let mut parts = s.split('.');
    let int = parts.next()?;
    let frac = parts.next();
    if parts.next().is_some() || int.is_empty() || int.len() > 2 {
        return None;
    }
    if int.len() == 2 && int.starts_with('0') {
        return None;
    }
    let digits = |p: &str| p.chars().all(|c| c.is_ascii_digit());
    let hundredths = match frac {
        None => 0,
        Some(f) if (1..=2).contains(&f.len()) && digits(f) => {
            format!("{f:0<2}").parse::<u32>().ok()?
        }
        Some(_) => return None,
    };
    let value = int.parse::<u32>().ok()? * 100 + hundredths;
    (digits(int) && value > 0).then_some(value)
}

#[test]
fn vat_percent_is_proven_over_every_short_string() {
    const ALPHABET: &[u8] = b"0123456789.";
    let mut strings = vec![String::new()];
    let mut frontier = strings.clone();
    for _ in 0..5 {
        frontier = frontier
            .iter()
            .flat_map(|s| ALPHABET.iter().map(move |&c| format!("{s}{}", c as char)))
            .collect();
        strings.extend(frontier.iter().cloned());
    }
    let mut accepted = 0;
    for s in &strings {
        let parsed = VatPercent::parse(s);
        match reference_percent(s) {
            Some(v) => {
                let shown = format!("{}.{:02}", v / 100, v % 100);
                assert_eq!(parsed.unwrap().to_string(), shown, "{s:?}");
                accepted += 1;
            }
            None => assert!(parsed.is_err(), "{s:?}"),
        }
    }
    // 100 integer parts × 111 fraction forms, less "0", "0.0" and "0.00".
    assert_eq!(accepted, 11_097);
}

#[test]
fn vat_percent_rejects_what_the_alphabet_cannot_spell() {
    for s in ["+25", "-1", " 25", "25 ", "1e1", "25,00", "٢٥"] {
        assert!(VatPercent::parse(s).is_err(), "{s:?}");
    }
}

#[test]
fn vat_percent_deserialises_from_a_string_only() {
    let p: VatPercent = serde_json::from_str(r#""25""#).unwrap();
    assert_eq!(p, VatPercent::parse("25.00").unwrap());
    assert!(serde_json::from_str::<VatPercent>("25").is_err());
    assert!(serde_json::from_str::<VatPercent>("25.0").is_err());
}

#[test]
fn vat_percent_serialises_at_scale_two_and_round_trips() {
    let p: VatPercent = serde_json::from_str(r#""25""#).unwrap();
    assert_eq!(serde_json::to_string(&p).unwrap(), r#""25.00""#);
    let back: VatPercent = serde_json::from_str(r#""25.00""#).unwrap();
    assert_eq!(back, p);
}

/// `bp` hundredths of a percent: `2500` is `25.00`.
fn pct(bp: i64) -> VatPercent {
    VatPercent::parse(&Decimal::new(bp, 2).to_string()).unwrap()
}

fn table() -> SeRates<VatPercent> {
    SeRates {
        standard: pct(3100),
        reduced12: pct(1700),
        reduced6: pct(300),
    }
}

fn line(ore: i64, rate: VatRate) -> SaleLine {
    SaleLine::new(
        "x".to_owned(),
        Money::from_ore(ore),
        rate,
        SupplyKind::Goods,
    )
    .unwrap()
}

fn vat(ore: i64, rate: VatRate) -> i128 {
    line_vat(&line(ore, rate), &table()).unwrap().ore()
}

/// `net × bp / 10 000` in öre, half away from zero. Net is never negative
/// here, so half-away is half-up.
fn reference(net: i64, bp: i64) -> i128 {
    (2 * i128::from(net) * i128::from(bp) + 10_000) / 20_000
}

#[test]
fn each_swedish_class_reads_its_own_key() {
    assert_eq!(vat(10_000, VatRate::Standard), 3_100);
    assert_eq!(vat(10_000, VatRate::Reduced12), 1_700);
    assert_eq!(vat(10_000, VatRate::Reduced6), 300);
}

/// 19 % is nowhere in the table: the number comes from the line.
#[test]
fn a_destination_line_uses_its_own_percentage() {
    assert_eq!(vat(10_000, VatRate::Destination(pct(1900))), 1_900);
}

/// Hand-worked, docs/01 §1.1 first: 40 000 kr at 25 % carries 10 000 kr.
/// Then 0.5 öre up, 0.06 öre down, and 1.5 öre up.
#[test]
fn line_vat_matches_hand_worked_cases() {
    let at = |ore, bp| vat(ore, VatRate::Destination(pct(bp)));
    assert_eq!(at(4_000_000, 2500), 1_000_000);
    assert_eq!(at(2, 2500), 1);
    assert_eq!(at(1, 600), 0);
    assert_eq!(at(25, 600), 2);
}

/// Two 2-öre lines at 25 % carry 1 öre each. Rounded on the 4-öre sum, the
/// same order would carry 1 öre in total (docs/02 §2.1).
#[test]
fn rounding_per_line_differs_from_rounding_the_sum() {
    let at_25 = VatRate::Destination(pct(2500));
    assert_eq!(vat(2, at_25) + vat(2, at_25), 2);
    assert_eq!(vat(4, at_25), 1);
}

/// Every percentage `VatPercent` can hold, including at the largest net, so
/// `line_vat`'s `Err` is unreachable and nothing overflows.
#[test]
fn line_vat_is_proven_over_every_percentage() {
    for bp in 1..=9_999 {
        for net in [0, 1, 2, 3, 25, 49, 50, 99, i64::MAX] {
            assert_eq!(
                vat(net, VatRate::Destination(pct(bp))),
                reference(net, bp),
                "{net} öre at {bp}"
            );
        }
    }
}

proptest! {
    #[test]
    fn line_vat_agrees_with_the_reference(net in 0..=i64::MAX, bp in 1..=9_999i64) {
        let rate = VatRate::Destination(pct(bp));
        prop_assert_eq!(vat(net, rate), reference(net, bp));
    }

    #[test]
    fn a_swedish_class_agrees_with_the_reference(net in 0..=i64::MAX) {
        prop_assert_eq!(vat(net, VatRate::Standard), reference(net, 3100));
        prop_assert_eq!(vat(net, VatRate::Reduced12), reference(net, 1700));
        prop_assert_eq!(vat(net, VatRate::Reduced6), reference(net, 300));
    }

    #[test]
    fn a_zero_rated_line_carries_no_vat(net in 0..=i64::MAX) {
        prop_assert_eq!(vat(net, VatRate::Zero), 0);
    }
}

// ── Territory tables ─────────────────────────────────────────────────────

fn code(s: &str) -> CountryCode {
    CountryCode::parse(s).unwrap()
}

/// Every value a `CountryCode` can hold: 26 × 26 = 676.
fn every_code() -> Vec<CountryCode> {
    let letters = || (b'A'..=b'Z').map(char::from);
    letters()
        .flat_map(|a| letters().map(move |b| code(&format!("{a}{b}"))))
        .collect()
}

/// The EU's member states, restated here and not read from `vat.rs`.
const MEMBER_STATES: [&str; 27] = [
    "SE", "DK", "FI", "DE", "FR", "IT", "ES", "PT", "NL", "BE", "LU", "IE", "AT", "GR", "CY", "MT",
    "PL", "CZ", "SK", "HU", "SI", "HR", "RO", "BG", "EE", "LV", "LT",
];

#[test]
fn the_vat_area_is_the_member_states_plus_monaco_and_northern_ireland() {
    let mut expected: Vec<CountryCode> = MEMBER_STATES.iter().map(|s| code(s)).collect();
    expected.extend([code("MC"), code("XI")]);
    for c in every_code() {
        assert_eq!(in_eu_vat_area(c), expected.contains(&c), "{c}");
    }
    let inside = every_code().into_iter().filter(|c| in_eu_vat_area(*c));
    assert_eq!(inside.count(), 29);
}

#[test]
fn a_territory_outside_the_vat_area_is_outside_whoever_it_belongs_to() {
    // Åland, five French overseas departments, the Faroes and Greenland, then
    // the United Kingdom and its dependencies, then neighbours and the US.
    for s in [
        "AX", "GP", "MQ", "GF", "RE", "YT", "FO", "GL", "GB", "IM", "JE", "GG", "GI", "NO", "CH",
        "IS", "LI", "AD", "SM", "VA", "US",
    ] {
        assert!(!in_eu_vat_area(code(s)), "{s}");
        assert!(is_assigned_country_code(code(s)), "{s}");
    }
}

/// How many assigned codes start with each letter, `A` to `Z`.
const BY_FIRST_LETTER: [usize; 26] = [
    16, 21, 19, 6, 7, 6, 19, 6, 10, 4, 11, 11, 23, 12, 1, 14, 1, 5, 21, 16, 6, 7, 2, 0, 2, 3,
];
/// How many assigned codes end with each letter, `A` to `Z`.
const BY_SECOND_LETTER: [usize; 26] = [
    12, 4, 8, 8, 15, 9, 13, 9, 10, 5, 9, 10, 17, 11, 12, 5, 5, 15, 12, 12, 9, 6, 10, 4, 9, 10,
];

#[test]
fn there_are_249_assigned_codes_with_the_expected_letter_counts() {
    let mut first = [0usize; 26];
    let mut second = [0usize; 26];
    let mut total = 0;
    for c in every_code() {
        if is_assigned_country_code(c) {
            let text = c.to_string();
            let [a, b] = text.as_bytes() else {
                panic!("{text} is not two bytes")
            };
            first[usize::from(a - b'A')] += 1;
            second[usize::from(b - b'A')] += 1;
            total += 1;
        }
    }
    assert_eq!(total, 249);
    assert_eq!(first, BY_FIRST_LETTER);
    assert_eq!(second, BY_SECOND_LETTER);
}

#[test]
fn a_code_iso_never_assigned_or_has_withdrawn_is_not_assigned() {
    // In use but never assigned, then placeholders, then withdrawn.
    for s in [
        "XI", "XK", "EL", "UK", "EU", "IC", "EA", "XX", "ZZ", "AA", "DD", "SU", "YU", "CS", "AN",
        "ZR",
    ] {
        assert!(!is_assigned_country_code(code(s)), "{s}");
    }
}

#[test]
fn northern_ireland_is_the_only_vat_area_code_that_is_not_assigned() {
    for c in every_code() {
        if in_eu_vat_area(c) {
            assert_eq!(is_assigned_country_code(c), c != code("XI"), "{c}");
        }
    }
}

// ── Scenario ─────────────────────────────────────────────────────────────

fn business() -> BuyerTaxStatus {
    BuyerTaxStatus::Business {
        vat_number: VatNumber::parse("DE123456789").unwrap(),
    }
}

/// A sale with no lines. `classify` reads the buyer and `ship_to` only.
fn sale(buyer: BuyerTaxStatus, ship_to: &str) -> SaleCaptured {
    SaleCaptured {
        id: EventId::parse("ord_1").unwrap(),
        at: NaiveDate::from_ymd_opt(2026, 3, 14).unwrap(),
        provider: ProviderId::parse("stripe").unwrap(),
        buyer,
        ship_to: code(ship_to),
        lines: Vec::new(),
    }
}

/// The error for a `ship_to` no scenario accepts, as `sale` would raise it.
fn unknown(ship_to: &str) -> PostingError {
    PostingError::UnknownShipTo {
        event: EventId::parse("ord_1").unwrap(),
        ship_to: code(ship_to),
    }
}

/// The docs/02 §3 conditions, restated. The VAT area is the list restated
/// above and not `in_eu_vat_area`. Whether a code is assigned does come from
/// `is_assigned_country_code`, which the letter counts above pin.
fn reference_scenario(buyer: &BuyerTaxStatus, ship_to: &str) -> Option<VatScenario> {
    if ship_to == "SE" {
        return Some(VatScenario::SeDomestic);
    }
    if MEMBER_STATES.contains(&ship_to) || ship_to == "MC" || ship_to == "XI" {
        return Some(match buyer {
            BuyerTaxStatus::Business { .. } => VatScenario::EuB2bReverse,
            BuyerTaxStatus::Consumer => VatScenario::EuB2cOss,
        });
    }
    is_assigned_country_code(code(ship_to)).then_some(VatScenario::ExportOutsideEu)
}

#[test]
fn classify_agrees_with_the_restated_conditions_on_every_code_and_buyer() {
    for c in every_code() {
        let s = c.to_string();
        for buyer in [BuyerTaxStatus::Consumer, business()] {
            let expected = reference_scenario(&buyer, &s).ok_or_else(|| unknown(&s));
            assert_eq!(classify(&sale(buyer, &s)), expected, "{s}");
        }
    }
}

/// How many of the 676 codes give each outcome, in the order domestic,
/// reverse charge, OSS, export, error.
fn tally(buyer: &BuyerTaxStatus) -> [usize; 5] {
    let mut counts = [0; 5];
    for c in every_code() {
        // Which error it is, is the test above's question.
        let slot = match classify(&sale(buyer.clone(), &c.to_string())).ok() {
            Some(VatScenario::SeDomestic) => 0,
            Some(VatScenario::EuB2bReverse) => 1,
            Some(VatScenario::EuB2cOss) => 2,
            Some(VatScenario::ExportOutsideEu) => 3,
            None => 4,
        };
        counts[slot] += 1;
    }
    counts
}

#[test]
fn each_outcome_covers_the_expected_number_of_codes() {
    // 29 codes in the VAT area, one of them Sweden. 249 assigned codes, of
    // which 28 are in the VAT area (`XI` is not assigned), so 221 exports.
    // 676 − 29 − 221 = 426 codes that name no country.
    assert_eq!(tally(&BuyerTaxStatus::Consumer), [1, 0, 28, 221, 426]);
    assert_eq!(tally(&business()), [1, 28, 0, 221, 426]);
}

#[test]
fn sweden_is_domestic_whoever_the_buyer_is() {
    // The business here has a German VAT number and is still domestic.
    for buyer in [BuyerTaxStatus::Consumer, business()] {
        assert_eq!(classify(&sale(buyer, "SE")), Ok(VatScenario::SeDomestic));
    }
}

#[test]
fn monaco_and_northern_ireland_go_by_the_buyer_like_a_member_state() {
    for s in ["DE", "GR", "MC", "XI"] {
        let to = |buyer| classify(&sale(buyer, s));
        assert_eq!(to(business()), Ok(VatScenario::EuB2bReverse), "{s}");
        assert_eq!(
            to(BuyerTaxStatus::Consumer),
            Ok(VatScenario::EuB2cOss),
            "{s}"
        );
    }
}

#[test]
fn an_assigned_code_outside_the_vat_area_is_an_export_for_either_buyer() {
    // Åland belongs to a member state and is still an export.
    // VERIFY (Q13a): `IM` is an export only on the reading that ML 2 kap.
    // 23 § treats a sale to the Isle of Man as a sale to the United Kingdom.
    for s in ["AX", "IM", "GB", "NO", "CH", "US"] {
        for buyer in [BuyerTaxStatus::Consumer, business()] {
            assert_eq!(
                classify(&sale(buyer, s)),
                Ok(VatScenario::ExportOutsideEu),
                "{s}"
            );
        }
    }
}

#[test]
fn a_code_that_names_no_country_is_an_error_and_never_an_export() {
    // Sent by platforms but never assigned (Kosovo, the Canary Islands, Ceuta
    // and Melilla), then common mistakes for `GB` and `GR`, then placeholders
    // and a withdrawn code.
    for s in ["XK", "IC", "EA", "UK", "EL", "EU", "XX", "ZZ", "DD"] {
        for buyer in [BuyerTaxStatus::Consumer, business()] {
            assert_eq!(classify(&sale(buyer, s)), Err(unknown(s)), "{s}");
        }
    }
}

proptest! {
    #[test]
    fn the_scenario_depends_on_the_kind_of_buyer_and_not_on_the_vat_number(
        number in ".+",
        index in 0..676usize,
    ) {
        let s = every_code()[index].to_string();
        let buyer = BuyerTaxStatus::Business {
            vat_number: VatNumber::parse(&number).unwrap(),
        };
        prop_assert_eq!(classify(&sale(buyer, &s)), classify(&sale(business(), &s)));
    }
}

// ── Display ──────────────────────────────────────────────────────────────

#[test]
fn a_rate_displays_as_its_wire_spelling() {
    // For the four classes, `Display` is the JSON string without its quotes.
    for rate in [
        VatRate::Standard,
        VatRate::Reduced12,
        VatRate::Reduced6,
        VatRate::Zero,
    ] {
        let wire = serde_json::to_string(&rate).unwrap();
        assert_eq!(format!("\"{rate}\""), wire);
    }
    // The OSS form is an object on the wire, and reads as words in a message.
    assert_eq!(
        VatRate::Destination(pct(1_900)).to_string(),
        "destination 19.00"
    );
}

#[test]
fn a_scenario_displays_as_its_docs_02_identifier() {
    let shown = [
        (VatScenario::SeDomestic, "SeDomestic"),
        (VatScenario::EuB2bReverse, "EuB2bReverse"),
        (VatScenario::EuB2cOss, "EuB2cOss"),
        (VatScenario::ExportOutsideEu, "ExportOutsideEu"),
    ];
    for (scenario, id) in shown {
        assert_eq!(scenario.to_string(), id);
    }
}

// ── Line check ───────────────────────────────────────────────────────────

/// `[vat.rates.oss]` for these tests, as country and hundredths of a percent.
/// The numbers are made up. No other country lists anything.
const LISTED: [(&str, i64); 3] = [("DE", 1_100), ("MC", 1_100), ("XI", 425)];

fn rates() -> VatRates {
    let mut oss: BTreeMap<CountryCode, BTreeSet<VatPercent>> = BTreeMap::new();
    for (country, bp) in LISTED {
        oss.entry(code(country)).or_default().insert(pct(bp));
    }
    VatRates { se: table(), oss }
}

/// Every form a line's rate can take against `rates()`: the four classes, a
/// percentage listed for `DE` and `MC`, and one listed for `XI` only.
fn rate_forms() -> [VatRate; 6] {
    [
        VatRate::Standard,
        VatRate::Reduced12,
        VatRate::Reduced6,
        VatRate::Zero,
        VatRate::Destination(pct(1_100)),
        VatRate::Destination(pct(425)),
    ]
}

fn sale_of(buyer: BuyerTaxStatus, ship_to: &str, lines: Vec<SaleLine>) -> SaleCaptured {
    SaleCaptured {
        lines,
        ..sale(buyer, ship_to)
    }
}

fn one_line(rate: VatRate, kind: SupplyKind) -> Vec<SaleLine> {
    vec![SaleLine::new("x".to_owned(), Money::from_ore(10_000), rate, kind).unwrap()]
}

/// Every sale with one line: 676 codes × 2 buyers × 6 rates × 2 kinds.
fn every_single_line_sale() -> Vec<(BuyerTaxStatus, String, VatRate, SupplyKind)> {
    let mut sales = Vec::new();
    for c in every_code() {
        for buyer in [BuyerTaxStatus::Consumer, business()] {
            for rate in rate_forms() {
                for kind in [SupplyKind::Goods, SupplyKind::Services] {
                    sales.push((buyer.clone(), c.to_string(), rate, kind));
                }
            }
        }
    }
    sales
}

/// What `check_sale` must answer for a sale whose only line is `(rate, kind)`.
/// This is the ADR-0006 §1 table restated by column, one rate at a time,
/// where `vat.rs` goes by row, one scenario at a time.
fn reference_check(
    buyer: &BuyerTaxStatus,
    ship_to: &str,
    rate: VatRate,
    kind: SupplyKind,
) -> Result<VatScenario, PostingError> {
    let event = || EventId::parse("ord_1").unwrap();
    let Some(scenario) = reference_scenario(buyer, ship_to) else {
        return Err(unknown(ship_to));
    };
    let gap = |reason| {
        Err(PostingError::UnsupportedScenario {
            event: event(),
            reason,
        })
    };
    let mismatch = Err(PostingError::VatRateMismatch {
        event: event(),
        line: 0,
        scenario,
        vat_rate: rate,
    });
    if kind == SupplyKind::Services && ship_to != "SE" {
        return gap(ScenarioGap::CrossBorderServices);
    }
    match rate {
        VatRate::Standard | VatRate::Reduced12 | VatRate::Reduced6 => {
            if scenario == VatScenario::SeDomestic {
                Ok(scenario)
            } else {
                mismatch
            }
        }
        VatRate::Zero => match scenario {
            VatScenario::SeDomestic => gap(ScenarioGap::DomesticZeroRate),
            VatScenario::EuB2cOss => gap(ScenarioGap::DestinationZeroRate),
            VatScenario::EuB2bReverse | VatScenario::ExportOutsideEu => Ok(scenario),
        },
        VatRate::Destination(percent) => {
            if scenario != VatScenario::EuB2cOss {
                mismatch
            } else if LISTED.contains(&(ship_to, hundredths(percent))) {
                Ok(scenario)
            } else {
                Err(PostingError::UnlistedOssRate {
                    event: event(),
                    line: 0,
                    country: code(ship_to),
                    percent,
                })
            }
        }
    }
}

/// A percentage back in hundredths: `11.00` is `1100`.
fn hundredths(percent: VatPercent) -> i64 {
    percent.to_string().replace('.', "").parse().unwrap()
}

#[test]
fn check_sale_agrees_with_the_restated_table_on_every_single_line_sale() {
    let rates = rates();
    let sales = every_single_line_sale();
    assert_eq!(sales.len(), 16_224);
    for (buyer, s, rate, kind) in sales {
        let expected = reference_check(&buyer, &s, rate, kind);
        let sale = sale_of(buyer, &s, one_line(rate, kind));
        assert_eq!(check_sale(&sale, &rates), expected, "{s} {rate} {kind:?}");
    }
}

#[test]
fn each_line_outcome_covers_the_expected_number_of_sales() {
    // In the order: passes, cross-border service, destination zero rate,
    // domestic zero rate, mismatch, unlisted rate, unknown `ship_to`.
    let rates = rates();
    let mut counts = [0; 7];
    for (buyer, s, rate, kind) in every_single_line_sale() {
        let slot = match check_sale(&sale_of(buyer, &s, one_line(rate, kind)), &rates) {
            Ok(_) => 0,
            Err(PostingError::UnsupportedScenario { reason, .. }) => match reason {
                ScenarioGap::CrossBorderServices => 1,
                ScenarioGap::DestinationZeroRate => 2,
                ScenarioGap::DomesticZeroRate => 3,
            },
            Err(PostingError::VatRateMismatch { .. }) => 4,
            Err(PostingError::UnlistedOssRate { .. }) => 5,
            Err(PostingError::UnknownShipTo { .. }) => 6,
            Err(
                other @ (PostingError::Money(_)
                | PostingError::Account(_)
                | PostingError::Verification(_)
                | PostingError::Event(_)),
            ) => panic!("{s}: {other}"),
        };
        counts[slot] += 1;
    }
    // Each code gives 12 sales per buyer. With both buyers:
    // - 426 codes name no country: 426 × 24 = 10 224 unknown.
    // - 249 codes are abroad, and a service there is a gap: 249 × 12 = 2 988.
    // - `SE`, goods or services: 3 classes pass, `zero` is a gap, 2
    //   destinations mismatch. × 4 = 12 pass, 4 gaps, 8 mismatches.
    // - 28 OSS codes, goods, consumer: 84 class mismatches, 28 zero gaps, and
    //   56 destinations of which `DE`, `MC` and `XI` list one each: 3 pass,
    //   53 unlisted.
    // - 28 codes, goods, business: `zero` passes 28 times, 140 mismatches.
    // - 221 exports, goods, both buyers: `zero` passes 442 times, 2 210
    //   mismatches.
    assert_eq!(counts, [485, 2_988, 28, 4, 2_442, 53, 10_224]);
}

#[test]
fn a_sale_with_no_lines_is_checked_as_classify_alone() {
    // Refusing an empty sale is the posting rule's job, not this check's.
    let rates = rates();
    for c in every_code() {
        for buyer in [BuyerTaxStatus::Consumer, business()] {
            let empty = sale(buyer, &c.to_string());
            assert_eq!(check_sale(&empty, &rates), classify(&empty), "{c}");
        }
    }
}

// ── Line check: several lines ────────────────────────────────────────────

/// `err` as it is reported when its line sits at `index`. A gap and an
/// unknown `ship_to` name no line, so they are unchanged.
fn at_line(err: PostingError, index: usize) -> PostingError {
    match err {
        PostingError::VatRateMismatch {
            event,
            scenario,
            vat_rate,
            ..
        } => PostingError::VatRateMismatch {
            event,
            line: index,
            scenario,
            vat_rate,
        },
        PostingError::UnlistedOssRate {
            event,
            country,
            percent,
            ..
        } => PostingError::UnlistedOssRate {
            event,
            line: index,
            country,
            percent,
        },
        other @ (PostingError::UnsupportedScenario { .. }
        | PostingError::UnknownShipTo { .. }
        | PostingError::Money(_)
        | PostingError::Account(_)
        | PostingError::Verification(_)
        | PostingError::Event(_)) => other,
    }
}

fn goods(rate: VatRate) -> SaleLine {
    line(10_000, rate)
}

fn service(rate: VatRate) -> SaleLine {
    SaleLine::new(
        "x".to_owned(),
        Money::from_ore(10_000),
        rate,
        SupplyKind::Services,
    )
    .unwrap()
}

fn gap(reason: ScenarioGap) -> PostingError {
    PostingError::UnsupportedScenario {
        event: EventId::parse("ord_1").unwrap(),
        reason,
    }
}

#[test]
fn a_sale_gives_the_outcome_of_its_first_failing_line() {
    // Every sale of up to three lines, each line in any of its 12 forms, to
    // nine codes of every kind: domestic, OSS destinations with and without
    // listed rates, exports, and one that names no country. What each line
    // gives alone is proven above. This proves how lines combine.
    let rates = rates();
    let forms: Vec<SaleLine> = rate_forms()
        .into_iter()
        .flat_map(|rate| [goods(rate), service(rate)])
        .collect();
    let mut sequences: Vec<Vec<usize>> = vec![Vec::new()];
    let mut frontier = sequences.clone();
    for _ in 0..3 {
        frontier = frontier
            .iter()
            .flat_map(|seq| (0..forms.len()).map(move |f| [seq.as_slice(), &[f]].concat()))
            .collect();
        sequences.extend(frontier.iter().cloned());
    }
    assert_eq!(sequences.len(), 1 + 12 + 144 + 1_728);

    let mut failed_at = [0; 3];
    for s in ["SE", "DE", "MC", "XI", "FR", "AX", "NO", "US", "XK"] {
        for buyer in [BuyerTaxStatus::Consumer, business()] {
            let scenario = classify(&sale(buyer.clone(), s));
            let alone: Vec<Result<VatScenario, PostingError>> = forms
                .iter()
                .map(|l| check_sale(&sale_of(buyer.clone(), s, vec![l.clone()]), &rates))
                .collect();
            for seq in &sequences {
                let first_failure = seq
                    .iter()
                    .enumerate()
                    .find_map(|(index, &f)| alone[f].clone().err().map(|err| (index, err)));
                let expected = match first_failure {
                    Some((index, err)) => {
                        failed_at[index] += 1;
                        Err(at_line(err, index))
                    }
                    None => scenario.clone(),
                };
                let lines = seq.iter().map(|&f| forms[f].clone()).collect();
                let got = check_sale(&sale_of(buyer.clone(), s, lines), &rates);
                assert_eq!(got, expected, "{s} {seq:?}");
            }
        }
    }
    // The first failure was seen at each of the three positions.
    assert!(failed_at.iter().all(|&n| n > 0), "{failed_at:?}");
}

#[test]
fn a_mismatch_and_an_unlisted_rate_name_their_line() {
    let rates = rates();
    let listed = || goods(VatRate::Destination(pct(1_100)));
    let to_germany = |lines| check_sale(&sale_of(BuyerTaxStatus::Consumer, "DE", lines), &rates);
    assert_eq!(
        to_germany(vec![listed(), listed(), goods(VatRate::Standard)]),
        Err(PostingError::VatRateMismatch {
            event: EventId::parse("ord_1").unwrap(),
            line: 2,
            scenario: VatScenario::EuB2cOss,
            vat_rate: VatRate::Standard,
        })
    );
    // 4.25 is listed for `XI` and not for `DE`.
    assert_eq!(
        to_germany(vec![listed(), goods(VatRate::Destination(pct(425)))]),
        Err(PostingError::UnlistedOssRate {
            event: EventId::parse("ord_1").unwrap(),
            line: 1,
            country: code("DE"),
            percent: pct(425),
        })
    );
}

#[test]
fn a_sale_mixing_goods_and_services_abroad_is_a_gap() {
    let rates = rates();
    let listed = || goods(VatRate::Destination(pct(1_100)));
    let unlisted = || goods(VatRate::Destination(pct(425)));
    let to_germany = |lines| check_sale(&sale_of(BuyerTaxStatus::Consumer, "DE", lines), &rates);
    let cross_border = Err(gap(ScenarioGap::CrossBorderServices));
    // The goods line is fine. The service makes the whole sale a gap.
    assert_eq!(
        to_germany(vec![listed(), service(VatRate::Standard)]),
        cross_border
    );
    assert_eq!(
        to_germany(vec![service(VatRate::Standard), unlisted()]),
        cross_border
    );
    // Lines are read in order, so a bad goods line ahead of the service wins.
    assert!(matches!(
        to_germany(vec![unlisted(), service(VatRate::Standard)]),
        Err(PostingError::UnlistedOssRate { line: 0, .. })
    ));
    // Within Sweden the same mix is an ordinary domestic sale.
    let domestic = vec![goods(VatRate::Standard), service(VatRate::Reduced12)];
    assert_eq!(
        check_sale(&sale_of(BuyerTaxStatus::Consumer, "SE", domestic), &rates),
        Ok(VatScenario::SeDomestic)
    );
}

#[test]
fn a_service_abroad_is_a_gap_whatever_rate_it_carries() {
    // Each rate here is right or wrong for its scenario's goods: a Swedish
    // class on an OSS sale, a listed percentage, `zero` on a reverse charge
    // and on an export. The service is refused first every time, as a gap
    // and not as the platform's mistake.
    let rates = rates();
    let listed_for_de = VatRate::Destination(pct(1_100));
    let listed_for_xi = VatRate::Destination(pct(425));
    let cases = [
        (BuyerTaxStatus::Consumer, "DE", VatRate::Standard),
        (BuyerTaxStatus::Consumer, "DE", listed_for_de),
        (BuyerTaxStatus::Consumer, "XI", listed_for_xi),
        (business(), "DE", VatRate::Zero),
        (BuyerTaxStatus::Consumer, "US", VatRate::Zero),
        (business(), "AX", VatRate::Standard),
    ];
    for (buyer, s, rate) in cases {
        assert_eq!(
            check_sale(&sale_of(buyer, s, vec![service(rate)]), &rates),
            Err(gap(ScenarioGap::CrossBorderServices)),
            "{s} {rate}"
        );
    }
}

#[test]
fn an_unknown_ship_to_is_reported_before_any_line() {
    // Every line here would fail on its own.
    let rates = rates();
    let lines = vec![service(VatRate::Zero), goods(VatRate::Destination(pct(1)))];
    assert_eq!(
        check_sale(&sale_of(BuyerTaxStatus::Consumer, "XK", lines), &rates),
        Err(unknown("XK"))
    );
}

#[test]
fn the_oss_table_can_refuse_a_line_and_never_admit_one() {
    let line = || vec![goods(VatRate::Destination(pct(1_100)))];
    // With nothing listed, no destination percentage passes anywhere.
    let none = VatRates {
        se: table(),
        oss: BTreeMap::new(),
    };
    assert!(matches!(
        check_sale(&sale_of(BuyerTaxStatus::Consumer, "DE", line()), &none),
        Err(PostingError::UnlistedOssRate { line: 0, .. })
    ));
    // `Config::new` refuses these keys. A table built by hand can hold them,
    // and they still let no destination percentage onto a sale that is not
    // an OSS sale.
    let mut oss = BTreeMap::new();
    for key in ["SE", "NO", "DE"] {
        oss.insert(code(key), BTreeSet::from([pct(1_100)]));
    }
    let by_hand = VatRates { se: table(), oss };
    for (buyer, s) in [
        (BuyerTaxStatus::Consumer, "SE"),
        (BuyerTaxStatus::Consumer, "NO"),
        (business(), "DE"),
    ] {
        assert!(
            matches!(
                check_sale(&sale_of(buyer, s, line()), &by_hand),
                Err(PostingError::VatRateMismatch { line: 0, .. })
            ),
            "{s}"
        );
    }
}

proptest! {
    #[test]
    fn the_amount_on_a_line_never_changes_the_outcome(
        ore in 0..=i64::MAX,
        form in 0..6usize,
        is_service in any::<bool>(),
        to in 0..9usize,
        is_business in any::<bool>(),
    ) {
        let rates = rates();
        let s = ["SE", "DE", "MC", "XI", "FR", "AX", "NO", "US", "XK"][to];
        let buyer = if is_business { business() } else { BuyerTaxStatus::Consumer };
        let rate = rate_forms()[form];
        let kind = if is_service { SupplyKind::Services } else { SupplyKind::Goods };
        let priced = SaleLine::new("x".to_owned(), Money::from_ore(ore), rate, kind).unwrap();
        prop_assert_eq!(
            check_sale(&sale_of(buyer.clone(), s, vec![priced]), &rates),
            check_sale(&sale_of(buyer, s, one_line(rate, kind)), &rates)
        );
    }
}
