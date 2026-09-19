//! `AccountNumber` and `AccountKind` — BAS chart primitives. See docs/02 §2.2.
//!
//! Parse, don't validate (docs/08 §2): an `AccountNumber` can only be built
//! from a value already inside the BAS range, so [`AccountNumber::class`] is
//! infallible. Nothing downstream re-checks the range.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What the BAS class of an account proves about it, and nothing more.
///
/// The variants are exactly the class line of docs/02 §2.2. Class 2 is not
/// split into equity and liabilities, and class 8 is not split into income and
/// expense: both splits need sub-range boundaries that the first digit cannot
/// prove, and nothing in v0.1 consumes them. If a consumer appears, the split
/// arrives as a sourced range table — not as a guess inside [`AccountNumber::class`].
///
/// Never add a wildcard arm over this enum (docs/08 §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AccountKind {
    /// Class 1 — assets (tillgångar).
    Asset,
    /// Class 2 — equity and liabilities (eget kapital och skulder).
    EquityAndLiability,
    /// Class 3 — income (intäkter).
    Income,
    /// Classes 4–7 — expenses (kostnader).
    Expense,
    /// Class 8 — financial items. Mixes income and expense, which is why it is
    /// folded into neither.
    FinancialItem,
}

/// A four-digit BAS account number, `1000`–`8999` (docs/02 §2.2).
///
/// The field is private and every constructor checks the range, so holding an
/// `AccountNumber` is proof of the range. Ordering is numeric, which is the
/// order a `BTreeMap` keyed on accounts iterates in — and therefore the order
/// accounts reach output (docs/03 §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AccountNumber(u16);

/// Why a string or integer could not become an [`AccountNumber`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccountError {
    /// Not exactly four ASCII digits.
    #[error("`{input}` is not a BAS account number; expected exactly four digits, e.g. \"1930\"")]
    Syntax {
        /// The rejected input, verbatim.
        input: String,
    },
    /// Four digits (or an integer), but outside `1000`–`8999`.
    #[error("`{input}` is outside the BAS account range 1000–8999")]
    OutOfRange {
        /// The rejected input, verbatim.
        input: String,
    },
}

impl AccountNumber {
    /// The lowest BAS account number.
    pub const MIN: AccountNumber = AccountNumber(1000);

    /// The highest BAS account number.
    pub const MAX: AccountNumber = AccountNumber(8999);

    /// Parse the config form (docs/04 §3): `"1930"`.
    ///
    /// Strict on purpose. Exactly four ASCII digits; no sign, no whitespace,
    /// no sub-account suffix, no non-ASCII digits.
    ///
    /// # Errors
    ///
    /// - [`AccountError::Syntax`] if the text is not exactly four ASCII digits
    /// - [`AccountError::OutOfRange`] if it is, but outside `1000`–`8999`
    pub fn parse(s: &str) -> Result<AccountNumber, AccountError> {
        s.parse()
    }

    /// The number as an integer, always within `1000..=8999`.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// The BAS class this account belongs to, read off the first digit.
    ///
    /// Infallible: the range was proven when the number was constructed.
    #[must_use]
    pub const fn class(self) -> AccountKind {
        // The two open-ended arms make the match exhaustive over `u16` with
        // neither a wildcard nor a panic. The type's invariant means they only
        // ever see 1000..=1999 and 8000..=8999.
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
/// is rejected: an account number is an identifier, not a quantity, and one
/// accepted form keeps host configs uniform.
impl<'de> Deserialize<'de> for AccountNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<AccountNumber, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    // Tests are the one place `unwrap()` is right: a wrong `Err` here is a test
    // failure, which is the point. The crate-level deny still holds for library code.
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn konto(s: &str) -> AccountNumber {
        AccountNumber::parse(s).unwrap()
    }

    fn syntax(s: &str) -> Result<AccountNumber, AccountError> {
        Err(AccountError::Syntax {
            input: s.to_owned(),
        })
    }

    fn out_of_range(s: &str) -> Result<AccountNumber, AccountError> {
        Err(AccountError::OutOfRange {
            input: s.to_owned(),
        })
    }

    #[test]
    fn parse_accepts_four_digits_in_the_bas_range() {
        assert_eq!(konto("1930").get(), 1930);
        assert_eq!(konto("1000"), AccountNumber::MIN);
        assert_eq!(konto("8999"), AccountNumber::MAX);
    }

    #[test]
    fn parse_rejects_anything_but_four_ascii_digits() {
        for s in [
            "",
            "158",
            "15800",
            " 1580",
            "1580 ",
            "+158",
            "-158",
            "15 0",
            "1e30",
            "158O",
            "1580.0",
            "١٩٣٠",
            "１９３０",
        ] {
            assert_eq!(AccountNumber::parse(s), syntax(s), "{s:?}");
        }
    }

    #[test]
    fn parse_rejects_four_digits_outside_the_bas_range() {
        for s in ["0000", "0999", "9000", "9999"] {
            assert_eq!(AccountNumber::parse(s), out_of_range(s), "{s:?}");
        }
    }

    #[test]
    fn try_from_u16_bounds_at_1000_and_8999() {
        assert_eq!(AccountNumber::try_from(1000), Ok(AccountNumber::MIN));
        assert_eq!(AccountNumber::try_from(8999), Ok(AccountNumber::MAX));
        assert_eq!(AccountNumber::try_from(999), out_of_range("999"));
        assert_eq!(AccountNumber::try_from(9000), out_of_range("9000"));
        assert_eq!(AccountNumber::try_from(0), out_of_range("0"));
        assert_eq!(AccountNumber::try_from(u16::MAX), out_of_range("65535"));
    }

    #[test]
    fn class_of_every_account_in_the_default_mapping() {
        // docs/02 §4. The numbers are that table's, not independent claims.
        for (s, kind) in [
            ("1580", AccountKind::Asset),
            ("1930", AccountKind::Asset),
            ("2611", AccountKind::EquityAndLiability),
            ("2612", AccountKind::EquityAndLiability),
            ("2613", AccountKind::EquityAndLiability),
            ("3001", AccountKind::Income),
            ("3002", AccountKind::Income),
            ("3003", AccountKind::Income),
            ("3105", AccountKind::Income),
            ("3106", AccountKind::Income),
            ("6570", AccountKind::Expense),
        ] {
            assert_eq!(konto(s).class(), kind, "{s}");
        }
    }

    #[test]
    fn display_is_the_sie_form_and_converts_back_to_u16() {
        assert_eq!(konto("1930").to_string(), "1930");
        assert_eq!(u16::from(konto("1930")), 1930);
    }

    #[test]
    fn serde_is_string_only() {
        assert_eq!(serde_json::to_string(&konto("1930")).unwrap(), r#""1930""#);
        assert_eq!(
            serde_json::from_str::<AccountNumber>(r#""1930""#).unwrap(),
            konto("1930")
        );
        assert!(
            serde_json::from_str::<AccountNumber>("1930").is_err(),
            "an account number is an identifier, not a quantity"
        );
        assert!(serde_json::from_str::<AccountNumber>(r#""9000""#).is_err());
        assert!(serde_json::from_str::<AccountNumber>(r#""193""#).is_err());
    }

    #[test]
    fn error_messages_say_what_was_wrong_and_what_is_expected() {
        // Error text is part of the UX (docs/05 §3.1), so it is pinned.
        assert_eq!(
            AccountNumber::parse("193").unwrap_err().to_string(),
            "`193` is not a BAS account number; expected exactly four digits, e.g. \"1930\""
        );
        assert_eq!(
            AccountNumber::parse("9000").unwrap_err().to_string(),
            "`9000` is outside the BAS account range 1000–8999"
        );
    }
}
