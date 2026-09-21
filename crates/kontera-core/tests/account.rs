//! Property tests for `AccountNumber` and `AccountKind`, over the public API
//! only.
//!
//! The integer domain is small enough (`u16`) to check *exhaustively*, so
//! those tests are plain loops over every value: a proof, not a sample.
//! `proptest` is used where the domain is unbounded — arbitrary strings.
//!
//! The recurring idea, as in `tests/money.rs`: each property has an
//! independent reference computation next to it. `class()` is implemented as a
//! range match on the integer; the reference reads the first character of the
//! displayed string.

use kontera_core::{AccountError, AccountKind, AccountNumber};
use proptest::prelude::*;

/// Reference for `parse`: what docs/02 §2.2 says, computed the slow way.
fn reference_parse(s: &str) -> Result<u16, AccountError> {
    let four_ascii_digits = s.chars().count() == 4 && s.chars().all(|c| c.is_ascii_digit());
    if !four_ascii_digits {
        return Err(AccountError::Syntax {
            input: s.to_owned(),
        });
    }
    match s.parse::<u16>() {
        Ok(n) if (1000..=8999).contains(&n) => Ok(n),
        _ => Err(AccountError::OutOfRange {
            input: s.to_owned(),
        }),
    }
}

/// Reference for `class`: the first displayed digit, per docs/02 §2.2.
fn reference_class(displayed: &str) -> Option<AccountKind> {
    match displayed.chars().next()? {
        '1' => Some(AccountKind::Asset),
        '2' => Some(AccountKind::EquityAndLiability),
        '3' => Some(AccountKind::Income),
        '4'..='7' => Some(AccountKind::Expense),
        '8' => Some(AccountKind::FinancialItem),
        _ => None,
    }
}

/// Exhaustive: an integer is an account number iff it is in `1000..=8999`
#[test]
fn try_from_u16_accepts_exactly_the_bas_range() {
    for n in 0..=u16::MAX {
        let got = AccountNumber::try_from(n);
        if (1000..=8999).contains(&n) {
            assert_eq!(got.map(AccountNumber::get), Ok(n), "{n}");
        } else {
            assert_eq!(
                got,
                Err(AccountError::OutOfRange {
                    input: n.to_string()
                }),
                "{n}"
            );
        }
    }
}

/// Exhaustive: every account displays as four digits and survives
/// display → parse and serialise → deserialise unchanged.
#[test]
fn every_account_round_trips_through_display_and_serde() {
    for n in AccountNumber::MIN.get()..=AccountNumber::MAX.get() {
        let a = AccountNumber::try_from(n).unwrap();
        let s = a.to_string();
        assert_eq!(s.len(), 4, "{s}");
        assert!(s.bytes().all(|b| b.is_ascii_digit()), "{s}");
        assert_eq!(AccountNumber::parse(&s), Ok(a), "{s}");
        assert_eq!(u16::from(a), n);

        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, format!("\"{s}\""));
        assert_eq!(serde_json::from_str::<AccountNumber>(&json).unwrap(), a);
    }
}

/// Exhaustive: `class()` is total over every constructible account ang agrees
/// with the first digit.
#[test]
fn class_agrees_with_the_first_digit_for_every_account() {
    for n in AccountNumber::MIN.get()..=AccountNumber::MAX.get() {
        let a = AccountNumber::try_from(n).unwrap();
        assert_eq!(Some(a.class()), reference_class(&a.to_string()), "{n}");
    }
}

/// Exhaustive: every four-digit string, including the ones with leading zeros
/// that no integer round-trip would ever produce.
#[test]
fn every_four_digit_string_parses_per_the_reference() {
    for n in 0..=9999u16 {
        let s = format!("{n:04}");
        assert_eq!(
            AccountNumber::parse(&s).map(AccountNumber::get),
            reference_parse(&s),
            "{s}"
        );
    }
}

proptest! {
    /// Arbitrary text never panics, and is accepted or rejected exactly as
    /// the reference says — with the input carried verbatim in the error.
    #[test]
    fn arbitrary_strings_parse_per_the_reference(s in "\\PC{0,8}") {
        prop_assert_eq!(
        AccountNumber::parse(&s).map(AccountNumber::get),
            reference_parse(&s)
    );
    }
    /// Digit strings of the wrong length are the near-misses a host is most
    /// likely to send (`"158"`, `"15800"`), so they get their own generator.
    #[test]
    fn digit_strings_parse_per_the_reference(s in "[0-9]{0,6}") {
        prop_assert_eq!(
        AccountNumber::parse(&s).map(AccountNumber::get),
            reference_parse(&s)
    );
    }

    /// Ordering is the integer ordering — what `BTreeMap` iteration, and so
    /// output order, will rely on.
    #[test]
    fn ordering_agrees_with_u16(a in 1000u16..=8999, b in 1000u16..=8999) {
        let (x, y) = (AccountNumber::try_from(a).unwrap(), AccountNumber::try_from(b).unwrap());
        prop_assert_eq!(x.cmp(&y), a.cmp(&b));
    }
}
