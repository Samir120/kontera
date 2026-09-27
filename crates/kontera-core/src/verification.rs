//! `Line`, `BalancedTransaction`, `Series` and `Verification`. See docs/02 §2.3.
//!
//! The private constructor is the thesis of the project: an unbalanced
//! verification cannot exist as a value in this system. A rule that produces
//! lines which do not sum to zero gets an `Err` at the point of construction,
//! not a wrong file at the point of export.

use std::fmt;
use std::str::FromStr;

use chrono::NaiveDate;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::account::AccountNumber;
use crate::money::Money;

/// One posting line: an account and a signed amount.
///
/// Debit is positive, credit is negative — the SIE `#TRANS` convention
/// (docs/04 §6). Both fields are already-proven types, so there is nothing
/// left for a constructor to check; build one with a struct literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    /// The BAS account posted to.
    pub account: AccountNumber,
    /// Debit positive, credit negative.
    pub amount: Money,
}

/// Why a set of lines could not become a [`BalancedTransaction`], or a string
/// could not become a [`Series`].
///
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerificationError {
    /// Debits and credits differ. Both totals are magnitudes; their difference
    /// is the amount a rule got wrong.
    #[error("verification would not balance: debit {debit}, credit {credit}")]
    Unbalanced {
        /// Sum of the positive lines.
        debit: Money,
        /// Sum of the negative lines, as a magnitude.
        credit: Money,
    },
    /// No lines at all. An empty sum is zero, but a `#VER` with no `#TRANS`
    /// documents no business event.
    #[error("verification has no lines")]
    Empty,
    /// Not exactly one ASCII capital letter.
    #[error("`{input}` is not a verification series; expected one capital letter, e.g. \"A\"")]
    InvalidSeries {
        /// The rejected input, verbatim
        input: String,
    },
}

/// A non-empty set of lines whose amounts sum to exactly zero.
///
/// The field is private and [`BalancedTransaction::new`] is the only
/// constructor, so holding one is proof of invariant **I1** (docs/02 §6).
/// Nothing downstream re-checks it.
///
/// Line order is pereserved as given. It is part of the output — the worked
/// example in docs/01 §1.1 lists the receivable before the income line, not
/// in account-number order — and the rule that built the lines decides it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalancedTransaction {
    lines: Vec<Line>,
}

impl BalancedTransaction {
    /// The only way in.
    ///
    /// # Errors
    ///
    /// - [`VerificationError::Empty`] if `lines` is empty
    /// - [`VerificationError::Unbalanced`] if the amounts do not sum to zero
    pub fn new(lines: Vec<Line>) -> Result<BalancedTransaction, VerificationError> {
        if lines.is_empty() {
            return Err(VerificationError::Empty);
        }
        // Total by ADR-0002: every amount entered through a `Money`
        // constructor, and this sums each of them once.
        let sum: Money = lines.iter().map(|line| line.amount).sum();
        if !sum.is_zero() {
            let (debit, credit) = totals(&lines);
            return Err(VerificationError::Unbalanced { debit, credit });
        }
        Ok(BalancedTransaction { lines })
    }

    /// The lines, in the order they were given, Never empty; sums to zero.
    #[must_use]
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }
}

/// Debit and credit magnitudes, summed separately.
fn totals(lines: &[Line]) -> (Money, Money) {
    lines
        .iter()
        .fold((Money::ZERO, Money::ZERO), |(debit, credit), line| {
            if line.amount.is_negative() {
                (debit, credit + line.amount.abs())
            } else {
                (debit + line.amount, credit)
            }
        })
}

/// A verification series (verifikationsserie): exactly one ASCII capital
/// letter, `A`-`Z`.
///
/// Strict on purpose, like every parser at this crate's boundary. docs/02 §1
/// says "a letter", the config form is `"A"` (docs/04 §3), and nothing in v0.1
/// needs more. Ordering is alphabetical, which is the order a `BTreeMap` keyed
/// on series iterates in.
/// VERIFY (Q4): the SIE `#VER` series field is a quoted string, and the spec
/// may permit more than one capital letter (digits, several letters). Widen
/// with evidence from the week-6 Fortnox import, not from memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Series(char);

impl Series {
    /// Parse the config from (docs/04 §3): `"A"`.
    ///
    /// # Errors
    ///
    /// [`VerificationError::InvalidSeries`] unless the text is exactly one
    /// ASCII capital letter.
    pub fn parse(s: &str) -> Result<Series, VerificationError> {
        s.parse()
    }

    /// the letter.
    #[must_use]
    pub const fn get(self) -> char {
        self.0
    }
}

impl FromStr for Series {
    type Err = VerificationError;

    fn from_str(s: &str) -> Result<Series, VerificationError> {
        // One byte that is an ASCII capital. A multi-byte character such as
        // `Å` is two or more bytes and falls through to the error arm.
        match s.as_bytes() {
            [b] if b.is_ascii_uppercase() => Ok(Series(char::from(*b))),
            [] | [_] | [_, _, ..] => Err(VerificationError::InvalidSeries {
                input: s.to_owned(),
            }),
        }
    }
}

/// The bare letter: `A`. This is the SIE `#VER` form, without its quotes.
impl fmt::Display for Series {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Serialised as a string, matching the config form.
impl Serialize for Series {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Accepts only a string in the [`Series::parse`] grammar.
impl<'de> Deserialize<'de> for Series {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Series, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// One verifikation (docs/02 §2.3): a balanced transaction with its palce in
/// the verification number series and its dates.
///
/// Plain data with public fields, on purpose. The one invariant at this level
/// that a type can carry — the lines balance — is carried by
/// [`BalancedTransaction`]. the other, **I4** (numbers sequential with no
/// gaps), is a property of the *sequence*, not of one verification, and is
/// enforced where the sequence is built: the ledger assembly stage
/// (docs/03 §4.1). Nothing here is read from a clock; both dates are inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verification {
    /// Which series the number belons to.
    pub series: Series,
    /// Position in the series. Assigned by the ledger builder, never by a rule.
    pub number: u32,
    /// The date of the business event — the SIE `verdatum`.
    pub date: NaiveDate,
    /// Free-text description — the SIE `vertext`.
    pub text: String,
    /// Teh date teh verification was compiled — the SIE `regdatum`. A
    /// parameter, never the system clock (docs/03 §4.2).
    pub posted: NaiveDate,
    /// The balanced lines.
    pub transaction: BalancedTransaction,
}

#[cfg(test)]
mod tests {
    // Tests are the place `unwrap()` is right: a wrong `Err` here is a test
    // failure, which is the point. The crate-level deny still holds for library code.
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn line(account: &str, kr: &str) -> Line {
        Line {
            account: AccountNumber::parse(account).unwrap(),
            amount: Money::parse(kr).unwrap(),
        }
    }

    fn kr(s: &str) -> Money {
        Money::parse(s).unwrap()
    }

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn the_sale_from_docs_01_balances() {
        let lines = vec![
            line("1580", "50000.00"),
            line("3001", "-40000.00"),
            line("2611", "-10000.00"),
        ];
        let bt = BalancedTransaction::new(lines.clone()).unwrap();
        assert_eq!(bt.lines(), lines.as_slice());
    }

    #[test]
    fn lines_keep_the_order_they_were_given() {
        // Not account-number: 1930 before 1580, as a payout rule writes it.
        let lines = vec![line("1930", "47832.00"), line("1580", "-47832.00")];
        let bt = BalancedTransaction::new(lines.clone()).unwrap();
        assert_eq!(bt.lines(), lines.as_slice());
    }

    #[test]
    fn one_ore_of_drift_is_unbalanced() {
        // The sale with output VAT rounded the wrong way by one äre.
        let err = BalancedTransaction::new(vec![
            line("1580", "50000.00"),
            line("3001", "-40000.00"),
            line("2611", "-10000.01"),
        ])
        .unwrap_err();
        assert_eq!(
            err,
            VerificationError::Unbalanced {
                debit: kr("50000.00"),
                credit: kr("50000.01"),
            }
        );
        assert_eq!(
            err.to_string(),
            "verification would not balance: debit 50000.00, credit 50000.01"
        );
    }

    #[test]
    fn a_payout_booked_without_clearing_the_receivable_is_unbalanced() {
        // Failure mode 1 from ocs/01 §1.2, caught at construction.
        let err = BalancedTransaction::new(vec![line("1930", "47832.00")]).unwrap_err();
        assert_eq!(
            err,
            VerificationError::Unbalanced {
                debit: kr("47832.00"),
                credit: Money::ZERO,
            }
        );
    }

    #[test]
    fn no_lines_is_an_error_not_a_balanced_nothing() {
        let err = BalancedTransaction::new(Vec::new()).unwrap_err();
        assert_eq!(err, VerificationError::Empty);
        assert_eq!(err.to_string(), "verification has no lines");
    }

    #[test]
    fn series_accepts_exactly_one_capital_letter() {
        assert_eq!(Series::parse("A").unwrap().get(), 'A');
        assert_eq!(Series::parse("Z").unwrap().get(), 'Z');
        assert_eq!(Series::parse("A").unwrap().to_string(), "A");
        assert!(Series::parse("A").unwrap() < Series::parse("B").unwrap());
    }

    #[test]
    fn series_rejects_anything_else_with_a_pinned_message() {
        for s in ["", "a", "1", "Å", " A", "A ", "\"A\""] {
            assert_eq!(
                Series::parse(s),
                Err(VerificationError::InvalidSeries {
                    input: s.to_owned()
                }),
                "{s:?}"
            );
        }
        assert_eq!(
            Series::parse("ab").unwrap_err().to_string(),
            "`ab` is not a verification series; expected one capital letter, e.g. \"A\""
        );
    }

    #[test]
    fn series_serde_is_string_only() {
        let a = Series::parse("A").unwrap();
        assert_eq!(serde_json::to_string(&a).unwrap(), r#""A""#);
        assert_eq!(serde_json::from_str::<Series>(r#""A""#).unwrap(), a);
        assert!(serde_json::from_str::<Series>(r#""a""#).is_err());
        assert!(serde_json::from_str::<Series>("65").is_err());
    }

    #[test]
    fn the_verification_from_docs_04_is_a_struct_literal() {
        // #VER "A" "1" 20260105 "Försäljning 2026-01-05" 20260108
        let v = Verification {
            series: Series::parse("A").unwrap(),
            number: 1,
            date: ymd(2026, 1, 5),
            text: "Försäljning 2026-01-05".to_owned(),
            posted: ymd(2026, 1, 8),
            transaction: BalancedTransaction::new(vec![
                line("1580", "50000.00"),
                line("3001", "-40000.00"),
                line("2611", "-10000.00"),
            ])
            .unwrap(),
        };
        assert_eq!(v.transaction.lines().len(), 3);
        assert!(v.date < v.posted);
        assert_eq!(v.clone(), v);
    }
}
