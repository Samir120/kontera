//! `AccountNumber` and `AccoundKind` — BAS chart primitives. See docs/02 §2.2.
//!
//! Parse, don't validate (docs/08 §2): an `AccountNumber` can only be built
//! from a value already inside the BAS range, so [`AccountNumber::class`] is
//! infallible. Nothing downstream re-check the range.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What the BAS class of an account proves about it, and nothing more.
///
/// The variants are exactly the class line of docs/02 §2.2. Class 2 is not
/// split into equity and liabilities, and class 8 is not split into income and
/// expense: both splits need sub-range boundaries that the first digit cannot
/// prove, and nothing in v0.1 consumes them. if a consumer appears, the split
/// arrives as a source range table — not as a guess inside [`AccountNumber::class`].
///
/// Never add a wildcard arm over this enum (docs/08 §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AccountKind {
    /// Class 1 — assets (tillgångar).
    Asset,
    /// Class 2 — equity and liabilities (eget kapital och skulder)
    EquityAndLiability,
    /// Class 3 — income (intäkter).
    Income,
    /// Classes 4-7 — expenses (kostnader).
    Expense,
    /// Class 8 — financial items. Mixes income and expense, which is why it is
    /// folded into neither.
    FinancialItem,
}

/// A four-digit BAS account number, `1000`-`8999` (docs/02 §2.2).
///
/// The field is private and every constructor checks the range, so holding an
/// `AccountNumber` is proof of the range. Ordering is numeric, which is the
/// order a `BTreeMap` keyed on accounts iterate in — and therefore the order
/// accounts reach output (docs/03 §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AccountNumber(u16);

/// why a string or integer could not become an [`AccountNumber`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccountError {
    /// Not exactly four ASCII digits.
    #[error(
        "`{input}` is outside the BAS account number; expected exactly four digits, e.g. \"1930\""
    )]
    Syntax {
        /// The rejected input, verbatim.
        input: String,
    },
    /// Four digits (or an integer), but outside `1000`-`8999`.
    #[error("`{input}` is outside the BAS acount range 1000-8999")]
    OutOfRange {
        /// The rejected input, verbatim.
        input: String,
    },
}

impl AccountNumber {
    /// The lowes BAS account number.
    pub const MIN: AccountNumber = AccountNumber(1000);

    /// The highest BAS account number.
    pub const MAX: AccountNumber = AccountNumber(8999);

    /// Parse the config from (docs/04 §3): `"1930"`.
    ///
    /// Strict on purpose. Exactly four ASCII digits; no sign, no whitespace,
    /// no sub-account suffix, no no-ASCII digits.
    ///
    /// # Errors
    ///
    /// - [`AccountError::Syntax`] if the text is not exactly four ASCII digits
    /// - [`AccountError::OutOfRange`] if it is, but outside `1000`-`8999`
    pub fn parse(s: &str) -> Result<AccountNumber, AccountError> {
        s.parse()
    }

    /// The number as an integer, always withing `1000..=8999`.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// The BAS class this account belongs to, read off the first digit.
    ///
    /// infallible: the range was proven when the number was constructed.
    #[must_use]
    pub const fn class(self) -> AccountKind {
        // The two open-ended arms make the match exhaustive over `u16` with
        // neither a wildcard nor a panic. the type's invariant means they only
        // ever see 1000..=19999 and 8000..=8999.
        match self.0 {
            ..=1999 => AccountKind::Asset,
            2000..=2999 => AccountKind::EquityAndLiability,
            3000..=3999 => AccountKind::Income,
            4000..=7999 => AccountKind::Expense,
            8000.. => AccountKind::FinancialItem,
        }
    }

    /// The single range check every constructor goes through.
    fn in_range(n: u16) -> Option<AccountNumber> {
        (Self::MIN.0..=Self::MAX.0)
            .contains(&n)
            .then_some(AccountNumber(n))
    }
}

impl FromStr for AccountNumber {
    type Err = AccountError;

    fn from_str(s: &str) -> Result<AccountNumber, AccountError> {
        if s.len() != 4 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(AccountError::Syntax {
                input: s.to_owned(),
            });
        }
        // Four ASCII digits: at most 9999, so the fold cannot overflow `u16`.
        let n = s
            .bytes()
            .fold(0u16, |acc, b| acc * 10 + u16::from(b - b'0'));
        AccountNumber::in_range(n).ok_or_else(|| AccountError::OutOfRange {
            input: s.to_owned(),
        })
    }
}

impl TryFrom<u16> for AccountNumber {
    type Error = AccountError;

    fn try_from(n: u16) -> Result<AccountNumber, AccountError> {
        AccountNumber::in_range(n).ok_or_else(|| AccountError::OutOfRange {
            input: n.to_string(),
        })
    }
}

impl From<AccountNumber> for u16 {
    fn from(a: AccountNumber) -> u16 {
        a.0
    }
}

/// Four digits, no padding needed since the minimum is `1000`. This is the
/// SIE `#KONTO` / `#TRANS` form.
impl fmt::Display for AccountNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Serialised as a string, matching the config form in docs/04 §3.
impl Serialize for AccountNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Accepts only a string in the [`AccountNumber::parse`] grammar. An integer
/// is rejected: an account is an identifier, not a quantity, and one
/// accepted form keeps host configs uniform.
impl<'de> Deserialize<'de> for AccountNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<AccountNumber, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
