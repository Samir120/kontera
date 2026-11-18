//! `PostingError`, the error type of `post()`. See docs/04 §4.
//!
//! Errors are the product. Each variant carries data a host can branch on,
//! not prose.
//!
//! The per-module enums ([`MoneyError`], [`AccountError`], [`VerificationError`]
//! and [`EventError`]) nest into it via `#[from]`, so `?` converts at any
//! boundary without a `map_err`. Their messages are already self-describing,
//! so the nesting variants are `transparent`: `Display` and `source()` are
//! forwarded straight to the inner error, and a host printing the chain never
//! sees the same sentence twice.
//!
//! The leaf variants carry a failure that only the fold can detect. Each is
//! added with the code that first returns it, so every variant lands together
//! with a test that actually produces it. [`PostingError::UnknownShipTo`] is
//! the first, returned by `vat::classify`. Still to come from docs/04 §4:
//! `SettlementMismatch`, `UnsupportedScenario`, `UnknownReference`,
//! `DuplicateEventId`, `MissingAccountMapping`, `PeriodClosed` and
//! `OutsidePeriod`. `ScenarioGap` and `MappingKey` do not exist yet.

use crate::account::AccountError;
use crate::event::{CountryCode, EventError, EventId};
use crate::money::MoneyError;
use crate::verification::VerificationError;

/// Why an event log could not be folded into a ledger.
///
/// Every variant is data a host can match on. Matching is exhaustive by
/// crate policy (docs/08 §6): when a variant is added, every consumer's
/// `match` stops compiling, which is the point.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PostingError {
    /// An amount could not be parsed or represented in öre.
    #[error(transparent)]
    Money(#[from] MoneyError),
    /// An account number was malformed or outside the BAS range.
    #[error(transparent)]
    Account(#[from] AccountError),
    /// A verification would not balance, had no lines, or named an invalid
    /// series.
    #[error(transparent)]
    Verification(#[from] VerificationError),
    /// An event carried a malformed identifier or an amount of the wrong sign.
    #[error(transparent)]
    Event(#[from] EventError),
    /// A sale ships to a code that is neither in the EU VAT area nor an
    /// assigned ISO 3166-1 code, so no scenario can be chosen for it. It is
    /// never treated as an export.
    #[error("event {event} ships to `{ship_to}`, which is neither in the EU VAT area nor an assigned ISO 3166-1 country code; send the code of the VAT territory the goods ship to")]
    UnknownShipTo {
        /// The sale.
        event: EventId,
        /// The code as the host sent it.
        ship_to: CountryCode,
    },
}

#[cfg(test)]
mod tests {
    // Tests are the place `unwrap()` is right: a wrong `Err` here is a test
    // failure, which is the point. The crate-level deny still holds for library code.
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{AccountNumber, CountryCode, EventId, Money, Series};
    use std::error::Error;

    #[test]
    fn question_mark_converts_every_nested_error() {
        fn money() -> Result<Money, PostingError> {
            Ok(Money::parse("x")?)
        }
        fn account() -> Result<AccountNumber, PostingError> {
            Ok(AccountNumber::parse("x")?)
        }
        fn series() -> Result<Series, PostingError> {
            Ok(Series::parse("x")?)
        }
        fn country() -> Result<CountryCode, PostingError> {
            Ok(CountryCode::parse("x")?)
        }

        assert!(matches!(
            money(),
            Err(PostingError::Money(MoneyError::Syntax { .. }))
        ));
        assert!(matches!(
            account(),
            Err(PostingError::Account(AccountError::Syntax { .. }))
        ));
        assert!(matches!(
            series(),
            Err(PostingError::Verification(
                VerificationError::InvalidSeries { .. }
            ))
        ));
        assert!(matches!(
            country(),
            Err(PostingError::Event(EventError::InvalidCountryCode { .. }))
        ));
    }

    #[test]
    fn display_is_the_inner_message_verbatim() {
        let inner = VerificationError::Empty;
        let outer = PostingError::from(inner.clone());
        assert_eq!(outer.to_string(), inner.to_string());
        assert_eq!(outer.to_string(), "verification has no lines");
    }

    #[test]
    fn no_variant_repeats_a_message_in_its_source_chain() {
        // `transparent` forwards `source()` to the inner error's own source,
        // which is `None` for all four. If someone replaces `transparent`
        // with `#[error("{0}")]`, `source()` becomes `Some(inner)` and a host
        // walking the chain prints the same message twice. This pins it. A
        // leaf has nothing inside it, so its message is pinned in full.
        //
        // The match is exhaustive on purpose: a new variant must be added
        // here deliberately, with its own expectation.
        let cases = [
            PostingError::from(Money::parse("x").unwrap_err()),
            PostingError::from(AccountNumber::parse("x").unwrap_err()),
            PostingError::from(Series::parse("x").unwrap_err()),
            PostingError::from(CountryCode::parse("x").unwrap_err()),
            PostingError::UnknownShipTo {
                event: EventId::parse("ord_1").unwrap(),
                ship_to: CountryCode::parse("XK").unwrap(),
            },
        ];
        for err in cases {
            let expected = match &err {
                PostingError::Money(inner) => inner.to_string(),
                PostingError::Account(inner) => inner.to_string(),
                PostingError::Verification(inner) => inner.to_string(),
                PostingError::Event(inner) => inner.to_string(),
                PostingError::UnknownShipTo { .. } => "event ord_1 ships to `XK`, which is \
                     neither in the EU VAT area nor an assigned ISO 3166-1 country code; \
                     send the code of the VAT territory the goods ship to"
                    .to_owned(),
            };
            assert_eq!(err.to_string(), expected);
            assert!(err.source().is_none());
        }
    }
}
