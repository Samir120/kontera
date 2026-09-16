//! `Money` — decimal amounts in öre. See ADR-0002 and docs/02 §2.1.
//!
//! Rounding is half-away-from-zero, applied per line and then summed. Never
//! sum then round: the results differ, and only the per-line result matches
//! what the host's checkout charged.
//!
//! Two ways in, deliberately distinct. [`Money::parse`] and `TryFrom<Decimal>`
//! accept only amounts already exact in öre — host input fails closed on
//! anything that would need rounding. [`Money::rounded`] is the one place in
//! the crate where a computed decimal is rounded to öre.

use std::fmt;
use std::iter::Sum;
use std::ops::{Add, Neg, Sub};
use std::str::FromStr;

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Every `Money` has exactly this many decimal places: öre.
const SCALE: u32 = 2;

/// The currency a [`Money`] is denominated in.
///
/// v0.1 is SEK-only (docs/01 §4). The tag exists so that a second currency is
/// a change here plus every exhaustive `match` the compiler then lists — not a
/// search through the codebase. Never add a wildcard arm over it (docs/08 §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Currency {
    /// Swedish krona.
    Sek,
}

impl Currency {
    /// ISO 4217 code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Currency::Sek => "SEK",
        }
    }
}

/// An exact amount of money in öre, in one currency.
///
/// Invariants, established by every constructor and preserved by every
/// operation:
///
/// 1. the amount has exactly two decimal places
/// 2. zero is never negative
/// 3. an amount that *entered* through a constructor fits in `i64` öre
///
/// The third is what makes `+`, `-` and `sum()` total: `Decimal` panics on
/// overflow at roughly 7.9e28, so a sum of bounded values would need more than
/// eight billion operands to overflow — more `Money` values than a host can
/// hold in memory. This covers summing values that entered through a
/// constructor, which is the only arithmetic this crate performs. It does not
/// cover compounding a result in a loop, and no code in this crate may do so.
/// Sums may legitimately exceed `i64` öre, which is why [`Money::ore`] returns
/// `i128`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Money {
    amount: Decimal,
    currency: Currency,
}

/// Why a decimal or string could not become a [`Money`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    /// Not of the form `-?digits[.digits]`, e.g. `"1000.00"`.
    #[error("`{input}` is not a plain decimal amount; expected the form \"1000.00\"")]
    Syntax {
        /// The rejected input, verbatim.
        input: String,
    },
    /// More decimal places than öre can hold without rounding.
    #[error("`{input}` has {scale} decimal places; öre precision allows at most 2")]
    TooPrecise {
        /// The rejected input, verbatim.
        input: String,
        /// Decimal places the input actually needs.
        scale: u32,
    },
    /// Outside `±92 233 720 368 547 758.07` (`i64` öre).
    #[error("`{input}` is outside the representable range of ±i64 öre")]
    OutOfRange {
        /// The rejected input, verbatim.
        input: String,
    },
}

impl Money {
    /// Zero SEK.
    pub const ZERO: Money = Money {
        amount: Decimal::from_parts(0, 0, 0, false, SCALE),
        currency: Currency::Sek,
    };

    /// Exact öre, no rounding. The constructor for tests and fixtures.
    #[must_use]
    pub fn from_ore(ore: i64) -> Money {
        // `SCALE` is far below `Decimal::MAX_SCALE`, so `new` cannot panic.
        Money::normalised(Decimal::new(ore, SCALE))
    }

    /// Parse the wire form from the event schema (docs/04 §2): `"1000.00"`.
    ///
    /// Strict on purpose. Accepts `-?digits` with an optional `.digits` tail;
    /// rejects signs other than `-`, whitespace, thousands separators,
    /// underscores, exponents and a bare `.` — anything a host's own
    /// formatting could produce by accident.
    ///
    /// # Errors
    ///
    /// - [`MoneyError::Syntax`] if the text is not of that form
    /// - [`MoneyError::TooPrecise`] if it needs more than two decimal places
    /// - [`MoneyError::OutOfRange`] if it exceeds `i64` öre
    pub fn parse(s: &str) -> Result<Money, MoneyError> {
        s.parse()
    }

    /// Round a computed decimal to öre, half away from zero.
    ///
    /// This is the only rounding in the crate. Apply it per line and then sum
    /// the lines (ADR-0002); rounding a sum gives a different number.
    ///
    /// # Errors
    ///
    /// [`MoneyError::OutOfRange`] if the rounded value exceeds `i64` öre.
    pub fn rounded(d: Decimal) -> Result<Money, MoneyError> {
        let r = d.round_dp_with_strategy(SCALE, RoundingStrategy::MidpointAwayFromZero);
        Money::bounded(r).ok_or_else(|| MoneyError::OutOfRange {
            input: d.to_string(),
        })
    }

    /// The currency tag.
    #[must_use]
    pub const fn currency(self) -> Currency {
        self.currency
    }

    /// The amount in öre. `i128` because sums may exceed what enters.
    #[must_use]
    pub const fn ore(self) -> i128 {
        self.amount.mantissa()
    }

    /// `true` for zero of any sign — though zero never carries a sign here.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.amount.is_zero()
    }

    /// `true` strictly below zero.
    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.amount.is_sign_negative()
    }

    /// Magnitude.
    #[must_use]
    pub fn abs(self) -> Money {
        Money::normalised(self.amount.abs())
    }

    /// Invariants 1 and 2 for a value already known to be in range.
    fn normalised(d: Decimal) -> Money {
        let mut amount = if d.is_zero() { Decimal::ZERO } else { d };
        amount.rescale(SCALE);
        Money {
            amount,
            currency: Currency::Sek,
        }
    }

    /// All three invariants. `None` if `d` does not fit in `i64` öre.
    fn bounded(d: Decimal) -> Option<Money> {
        let m = Money::normalised(d);
        // `rescale` refuses to widen past 96 bits and silently keeps a smaller
        // scale instead, so the scale check is not redundant with the range check.
        (m.amount.scale() == SCALE && i64::try_from(m.amount.mantissa()).is_ok()).then_some(m)
    }

    /// Shared tail of `parse` and `TryFrom<Decimal>`: exact in öre, in range.
    fn exact(d: Decimal, input: &dyn fmt::Display) -> Result<Money, MoneyError> {
        let exact = d.normalize();
        if exact.scale() > SCALE {
            return Err(MoneyError::TooPrecise {
                input: input.to_string(),
                scale: exact.scale(),
            });
        }
        Money::bounded(exact).ok_or_else(|| MoneyError::OutOfRange {
            input: input.to_string(),
        })
    }
}

/// `-?digits[.digits]`, ASCII only. The grammar of docs/04 §2 amounts.
fn is_plain_decimal(s: &str) -> bool {
    let unsigned = s.strip_prefix('-').unwrap_or(s);
    let all_digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    match unsigned.split_once('.') {
        Some((int, frac)) => all_digits(int) && all_digits(frac),
        None => all_digits(unsigned),
    }
}

impl FromStr for Money {
    type Err = MoneyError;

    fn from_str(s: &str) -> Result<Money, MoneyError> {
        if !is_plain_decimal(s) {
            return Err(MoneyError::Syntax {
                input: s.to_owned(),
            });
        }
        // The grammar is already checked, so the only way `Decimal` can refuse
        // a string here is magnitude.
        let d = Decimal::from_str(s).map_err(|_| MoneyError::OutOfRange {
            input: s.to_owned(),
        })?;
        Money::exact(d, &s)
    }
}

impl TryFrom<Decimal> for Money {
    type Error = MoneyError;

    fn try_from(d: Decimal) -> Result<Money, MoneyError> {
        Money::exact(d, &d)
    }
}

impl From<Money> for Decimal {
    fn from(m: Money) -> Decimal {
        m.amount
    }
}

/// Bare amount, two decimals, `.` separator, `-` for credit balances:
/// `"1234.56"`. No currency — this is the SIE `#TRANS` form.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.amount, f)
    }
}

/// Serialised as a JSON string, never a number (ADR-0002).
impl Serialize for Money {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Accepts only a JSON string in the [`Money::parse`] grammar. A JSON number
/// is rejected: it has already been a float in the host's parser.
impl<'de> Deserialize<'de> for Money {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Money, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl Add for Money {
    type Output = Money;

    fn add(self, rhs: Money) -> Money {
        match (self.currency, rhs.currency) {
            (Currency::Sek, Currency::Sek) => Money::normalised(self.amount + rhs.amount),
        }
    }
}

impl Sub for Money {
    type Output = Money;

    fn sub(self, rhs: Money) -> Money {
        match (self.currency, rhs.currency) {
            (Currency::Sek, Currency::Sek) => Money::normalised(self.amount - rhs.amount),
        }
    }
}

impl Neg for Money {
    type Output = Money;

    fn neg(self) -> Money {
        Money::normalised(-self.amount)
    }
}

impl Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Money {
        iter.fold(Money::ZERO, Add::add)
    }
}

impl<'a> Sum<&'a Money> for Money {
    fn sum<I: Iterator<Item = &'a Money>>(iter: I) -> Money {
        iter.copied().sum()
    }
}

#[cfg(test)]
mod tests {
    // Tests are the one place `unwrap()` is right: a wrong `Err` here is a test
    // failure, which is the point. The crate-level deny still holds for library code.
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn sek(ore: i64) -> Money {
        Money::from_ore(ore)
    }

    fn kr(s: &str) -> Money {
        Money::parse(s).unwrap()
    }

    fn dec(mantissa: i64, scale: u32) -> Decimal {
        Decimal::new(mantissa, scale)
    }

    #[test]
    fn parse_accepts_the_wire_form() {
        assert_eq!(Money::parse("1000.00"), Ok(sek(100_000)));
        assert_eq!(Money::parse("1000"), Ok(sek(100_000)));
        assert_eq!(Money::parse("0.5"), Ok(sek(50)));
        assert_eq!(Money::parse("-0.50"), Ok(sek(-50)));
        assert_eq!(Money::parse("1000.500"), Ok(sek(100_050)));
        assert_eq!(Money::parse("-0.00"), Ok(Money::ZERO));
    }

    #[test]
    fn parse_rejects_anything_but_the_wire_form() {
        for s in [
            "", "-", ".", "1.", ".5", "+1.00", " 1.00", "1.00 ", "1,000.00", "1_000.00", "1e3",
            "abc", "1.0.0",
        ] {
            assert_eq!(
                Money::parse(s),
                Err(MoneyError::Syntax {
                    input: s.to_owned()
                }),
                "{s:?}"
            );
        }
    }

    #[test]
    fn parse_rejects_sub_ore_precision_rather_than_rounding() {
        assert_eq!(
            Money::parse("1000.005"),
            Err(MoneyError::TooPrecise {
                input: "1000.005".to_owned(),
                scale: 3
            })
        );
    }

    #[test]
    fn parse_bounds_at_i64_ore() {
        assert_eq!(Money::parse("92233720368547758.07"), Ok(sek(i64::MAX)));
        assert_eq!(Money::parse("-92233720368547758.07"), Ok(sek(-i64::MAX)));
        for s in ["92233720368547758.08", "100000000000000000000000000000000"] {
            assert_eq!(
                Money::parse(s),
                Err(MoneyError::OutOfRange {
                    input: s.to_owned()
                }),
                "{s:?}"
            );
        }
    }

    #[test]
    fn try_from_decimal_is_exact_or_nothing() {
        assert_eq!(Money::try_from(dec(1_000, 3)), Ok(sek(100)));
        assert_eq!(
            Money::try_from(dec(1_005, 3)),
            Err(MoneyError::TooPrecise {
                input: "1.005".to_owned(),
                scale: 3
            })
        );
    }

    #[test]
    fn rounded_is_half_away_from_zero() {
        assert_eq!(Money::rounded(dec(125, 3)), Ok(sek(13)));
        assert_eq!(Money::rounded(dec(-125, 3)), Ok(sek(-13)));
        assert_eq!(Money::rounded(dec(124, 3)), Ok(sek(12)));
        assert_eq!(Money::rounded(dec(5, 3)), Ok(sek(1)));
        assert_eq!(Money::rounded(dec(-5, 3)), Ok(sek(-1)));
        assert_eq!(Money::rounded(dec(2_675, 3)), Ok(sek(268)));
    }

    #[test]
    fn per_line_rounding_then_sum_differs_from_sum_then_rounding() {
        // Ten lines of 1.25 kr at 6 % moms. Per line: 0.075 -> 0.08 each.
        let per_line: Money = (0..10)
            .map(|_| Money::rounded(dec(75, 3)))
            .sum::<Result<_, _>>()
            .unwrap();
        // Sum then round: 12.50 * 6 % = 0.75 exactly.
        let sum_then_round = Money::rounded(dec(750, 3)).unwrap();
        assert_eq!(per_line, sek(80));
        assert_eq!(sum_then_round, sek(75));
        assert_ne!(per_line, sum_then_round);
    }

    #[test]
    fn worked_example_from_docs_01_ties_out() {
        // 40 000 kr excl. moms at 25 %.
        let vat = Money::rounded(Decimal::from(kr("40000.00")) * dec(25, 2)).unwrap();
        assert_eq!(vat, kr("10000.00"));
        // Account 1580 after the payout: sales − refunds − fees − payout.
        let clearing = [
            kr("50000.00"),
            -kr("1000.00"),
            -kr("1168.00"),
            -kr("47832.00"),
        ];
        assert_eq!(clearing.iter().sum::<Money>(), Money::ZERO);
    }

    #[test]
    fn display_is_the_sie_trans_form() {
        assert_eq!(sek(123_456).to_string(), "1234.56");
        assert_eq!(sek(-5).to_string(), "-0.05");
        assert_eq!(sek(100).to_string(), "1.00");
        assert_eq!(Money::ZERO.to_string(), "0.00");
        assert_eq!(Money::parse("7").unwrap().to_string(), "7.00");
    }

    #[test]
    fn zero_is_never_negative() {
        for z in [
            Money::ZERO,
            -Money::ZERO,
            sek(5) - sek(5),
            Money::parse("-0.00").unwrap(),
        ] {
            assert!(!z.is_negative());
            assert!(z.is_zero());
            assert_eq!(z.to_string(), "0.00");
        }
    }

    #[test]
    fn arithmetic_and_ordering() {
        assert_eq!(sek(150) + sek(-50), sek(100));
        assert_eq!(sek(100) - sek(150), sek(-50));
        assert_eq!(-sek(100), sek(-100));
        assert_eq!(sek(-100).abs(), sek(100));
        assert_eq!(sek(100).ore(), 100);
        assert!(sek(-1) < Money::ZERO && Money::ZERO < sek(1));
        assert_eq!(Vec::<Money>::new().into_iter().sum::<Money>(), Money::ZERO);
    }

    #[test]
    fn serde_is_string_only() {
        assert_eq!(
            serde_json::to_string(&sek(123_456)).unwrap(),
            r#""1234.56""#
        );
        assert_eq!(
            serde_json::from_str::<Money>(r#""1234.56""#).unwrap(),
            sek(123_456)
        );
        assert!(
            serde_json::from_str::<Money>("1234.56").is_err(),
            "a JSON number is a float"
        );
        assert!(serde_json::from_str::<Money>(r#""1234.567""#).is_err());
        assert!(serde_json::from_str::<Money>(r#""1_234.56""#).is_err());
    }
}
