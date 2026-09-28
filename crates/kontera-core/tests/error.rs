//! Property tests for `PostingError` over the public API only.
//!
//! The nesting variants are `transparent`, so for *every* input a parser
//! rejects, teh error seen through `PostingError` must be the same value with
//! the same message as the error the parser returned. Arbitrary strings hit
//! every variant of every nested enum.

use kontera_core::{AccountNumber, Money, PostingError, Series};
use proptest::prelude::*;

proptest! {
    #[test]
    fn money_error_is_unchanged_through_posting_error(s in ".*") {
        if let Err(inner) = Money::parse(&s) {
            let outer = PostingError::from(inner.clone());
            prop_assert_eq!(outer.to_string(), inner.to_string());
            prop_assert_eq!(outer, PostingError::Money(inner));
        }
    }

    #[test]
    fn account_error_is_unchanged_through_posting_error(s in ".*") {
        if let Err(inner) = AccountNumber::parse(&s) {
            let outer = PostingError::from(inner.clone());
            prop_assert_eq!(outer.to_string(), inner.to_string());
            prop_assert_eq!(outer, PostingError::Account(inner));
        }
    }

    #[test]
    fn verification_error_is_unchanged_through_posting_error(s in ".*") {
        if let Err(inner) = Series::parse(&s) {
            let outer = PostingError::from(inner.clone());
            prop_assert_eq!(outer.to_string(), inner.to_string());
            prop_assert_eq!(outer,  PostingError::Verification(inner));
        }
    }
}
