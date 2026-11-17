//! Tests for `VatPercent`, `line_vat` and the territory tables, over the
//! public API only.
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

use kontera_core::{
    in_eu_vat_area, is_assigned_country_code, line_vat, CountryCode, Money, SaleLine, SeRates,
    SupplyKind, VatPercent, VatRate,
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
