//! Property tests for `Money`, over the public API only.
//!
//! `properties.rs` is reserved for the invariant suite that needs
//! `valid_event_log()` (T6). These need nothing but `Money` itself.
//!
//! The recurring idea: `Money` must agree with exact integer arithmetic on
//! öre. Every property below has an `i128` reference computation next to it,
//! which is what a float-based implementation would fail.

use kontera_core::{Money, MoneyError};
use proptest::prelude::*;
use rust_decimal::Decimal;

/// Reference rounding: `m × 10^-scale` kronor to öre, half away from zero,
/// in plain integer arithmetic.
fn round_half_away(m: i64, scale: u32) -> i128 {
    let m = i128::from(m);
    if scale <= 2 {
        return m * 10i128.pow(2 - scale);
    }
    let div = 10i128.pow(scale - 2);
    let (q, r) = (m.abs() / div, m.abs() % div);
    let q = if 2 * r >= div { q + 1 } else { q };
    if m < 0 {
        -q
    } else {
        q
    }
}

/// Mantissas that stay inside `i64` öre even at scale 0 (× 100).
fn in_range_mantissa() -> impl Strategy<Value = i64> {
    -(i64::MAX / 100)..=(i64::MAX / 100)
}

proptest! {
    /// Display is the SIE `#TRANS` form and parses back to the same value.
    #[test]
    fn display_round_trips_through_parse(ore in any::<i64>()) {
        let m = Money::from_ore(ore);
        let s = m.to_string();
        let (_, frac) = s.rsplit_once('.').expect("two decimals always present");
        prop_assert_eq!(frac.len(), 2, "{}", s);
        prop_assert!(frac.bytes().all(|b| b.is_ascii_digit()), "{}", s);
        prop_assert_eq!(Money::parse(&s), Ok(m));
        prop_assert_eq!(m.ore(), i128::from(ore));
    }

    /// JSON form is a string, and it round-trips.
    #[test]
    fn serde_round_trips_as_a_string(ore in any::<i64>()) {
        let m = Money::from_ore(ore);
        let json = serde_json::to_string(&m).unwrap();
        prop_assert!(json.starts_with('"') && json.ends_with('"'), "{}", json);
        prop_assert_eq!(serde_json::from_str::<Money>(&json).unwrap(), m);
    }

    /// `+`, `-`, unary `-` agree with `i128` öre arithmetic — including sums
    /// past `i64` öre, which is why `ore()` is `i128`.
    #[test]
    fn arithmetic_is_exact(a in any::<i64>(), b in any::<i64>()) {
        let (ma, mb) = (Money::from_ore(a), Money::from_ore(b));
        let (ia, ib) = (i128::from(a), i128::from(b));
        prop_assert_eq!((ma + mb).ore(), ia + ib);
        prop_assert_eq!((ma - mb).ore(), ia - ib);
        prop_assert_eq!((-ma).ore(), -ia);
        prop_assert_eq!(ma + mb - mb, ma);
        prop_assert_eq!(-(-ma), ma);
        prop_assert_eq!(ma.abs().ore(), ia.abs());
    }

    /// A sum of many values is the integer sum of their öre, by value and by
    /// reference.
    #[test]
    fn sum_is_exact(ore in prop::collection::vec(any::<i64>(), 0..64)) {
        let values: Vec<Money> = ore.iter().copied().map(Money::from_ore).collect();
        let expected: i128 = ore.iter().copied().map(i128::from).sum();
        prop_assert_eq!(values.iter().copied().sum::<Money>().ore(), expected);
        prop_assert_eq!(values.iter().sum::<Money>().ore(), expected);
    }

    /// Zero produced by arithmetic is `ZERO`: unsigned, and it displays as
    /// `0.00`, never `-0.00`.
    #[test]
    fn zero_from_arithmetic_is_never_negative(ore in any::<i64>()) {
        let m = Money::from_ore(ore);
        for z in [m - m, m + (-m), -(m - m)] {
            prop_assert_eq!(z, Money::ZERO);
            prop_assert!(!z.is_negative());
            prop_assert_eq!(z.to_string(), "0.00");
        }
    }

    /// `Money::rounded` is half-away-from-zero at every scale, matching the
    /// integer reference exactly.
    #[test]
    fn rounded_matches_the_integer_reference(m in in_range_mantissa(), scale in 0u32..=8) {
        let d = Decimal::new(m, scale);
        prop_assert_eq!(Money::rounded(d).map(Money::ore), Ok(round_half_away(m, scale)));
    }

        /// The exact constructors never round: anything with a non-zero digit past
    /// öre is `TooPrecise`, at the scale the input actually needs.
    #[test]
    fn exact_constructors_refuse_to_round(
        m in any::<i64>().prop_filter("last digit non-zero", |m| m % 10 != 0),
        scale in 3u32..=8,
    ) {
        let d = Decimal::new(m, scale);
        // Bound first: `prop_assert!` stringifies its condition into a format
        // string, and a struct pattern's braces would be read as placeholders.
        let via_decimal = Money::try_from(d);
        let via_parse = Money::parse(&d.to_string());
        let too_precise_at = |r: &Result<Money, MoneyError>| match r {
            Err(MoneyError::TooPrecise { scale, .. }) => Some(*scale),
            _ => None,
        };
        prop_assert_eq!(too_precise_at(&via_decimal), Some(scale), "{:?}", via_decimal);
        prop_assert_eq!(too_precise_at(&via_parse), Some(scale), "{:?}", via_parse);
    }

    /// docs/05 §2.2: many small lines at 6 %, rounded per line and summed,
    /// equal the per-line integer reference — and the total never drifts from
    /// the exact figure by more than half an öre per line.
    #[test]
    fn six_percent_lines_round_per_line_then_sum(
        lines in prop::collection::vec(1i64..=1_000_000, 1..200),
    ) {
        let six_percent = Decimal::new(6, 2);
        let total: Money = lines
            .iter()
            .map(|&ore| Money::rounded(Decimal::from(Money::from_ore(ore)) * six_percent).unwrap())
            .sum();
        // `ore × 6` is the VAT in hundredths of an öre: kronor at scale 4.
        let expected: i128 = lines.iter().map(|&ore| round_half_away(ore * 6, 4)).sum();
        prop_assert_eq!(total.ore(), expected);

        let exact_hundredths: i128 = lines.iter().map(|&ore| i128::from(ore) * 6).sum();
        let drift = (total.ore() * 100 - exact_hundredths).abs();
        prop_assert!(drift <= 50 * lines.len() as i128, "drift {} over {} lines", drift, lines.len());
    }
}
