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
//! with a test that actually produces it. `vat::classify` returns
//! [`PostingError::UnknownShipTo`]. `vat::check_sale` returns
//! [`PostingError::UnsupportedScenario`], [`PostingError::VatRateMismatch`]
//! and [`PostingError::UnlistedOssRate`]. Still to come from docs/04 §4:
//! `SettlementMismatch`, `UnknownReference`, `DuplicateEventId`,
//! `MissingAccountMapping`, `PeriodClosed` and `OutsidePeriod`. `MappingKey`
//! does not exist yet.

use crate::account::AccountError;
use crate::event::{CountryCode, EventError, EventId, VatRate};
use crate::money::MoneyError;
use crate::vat::{ScenarioGap, VatPercent, VatScenario};
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
    /// A sale needs a VAT treatment that v0.1 recognises and refuses
    /// (ADR-0004). The event is right, and the merchant books the order by
    /// hand.
    #[error("unsupported VAT scenario for event {event}: {reason}; v0.1 cannot post it, so book this order by hand")]
    UnsupportedScenario {
        /// The sale.
        event: EventId,
        /// Which treatment is missing.
        reason: ScenarioGap,
    },
    /// A line carries a `vat_rate` that the scenario of its sale can't take
    /// (ADR-0006 §1). The host sent the buyer, the `ship_to` or the rate
    /// wrong, so this is not reported as a gap.
    #[error("event {event}, lines[{line}]: vat_rate `{vat_rate}` does not fit a {scenario} sale; SeDomestic takes a Swedish class, EuB2cOss a destination percentage, EuB2bReverse and ExportOutsideEu `zero`")]
    VatRateMismatch {
        /// The sale.
        event: EventId,
        /// The line's index in the sale's `lines`, counted from zero.
        line: usize,
        /// The scenario the sale's buyer and `ship_to` give.
        scenario: VatScenario,
        /// The rate as the host sent it.
        vat_rate: VatRate,
    },
    /// An OSS line carries a percentage that `[vat.rates.oss]` does not list
    /// for the sale's `ship_to` (ADR-0006 §3). A country with no entry has no
    /// valid rates.
    #[error("event {event}, lines[{line}]: {percent} % is not listed for `{country}` in [vat.rates.oss]; correct the line, or add the rate if the checkout charged it")]
    UnlistedOssRate {
        /// The sale.
        event: EventId,
        /// The line's index in the sale's `lines`, counted from zero.
        line: usize,
        /// The sale's `ship_to`, which is the key looked up.
        country: CountryCode,
        /// The percentage on the line.
        percent: VatPercent,
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
        // Both matches are exhaustive on purpose: a new variant, or a new
        // gap, must be added here deliberately, with its own expectation.
        let event = || EventId::parse("ord_1").unwrap();
        let gap = |reason| PostingError::UnsupportedScenario {
            event: event(),
            reason,
        };
        let cases = [
            PostingError::from(Money::parse("x").unwrap_err()),
            PostingError::from(AccountNumber::parse("x").unwrap_err()),
            PostingError::from(Series::parse("x").unwrap_err()),
            PostingError::from(CountryCode::parse("x").unwrap_err()),
            PostingError::UnknownShipTo {
                event: event(),
                ship_to: CountryCode::parse("XK").unwrap(),
            },
            gap(ScenarioGap::CrossBorderServices),
            gap(ScenarioGap::DestinationZeroRate),
            gap(ScenarioGap::DomesticZeroRate),
            PostingError::VatRateMismatch {
                event: event(),
                line: 1,
                scenario: VatScenario::EuB2cOss,
                vat_rate: VatRate::Standard,
            },
            PostingError::UnlistedOssRate {
                event: event(),
                line: 0,
                country: CountryCode::parse("DE").unwrap(),
                percent: VatPercent::parse("19").unwrap(),
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
                PostingError::UnsupportedScenario { reason, .. } => {
                    let treatment = match reason {
                        ScenarioGap::CrossBorderServices => "a service sold outside Sweden",
                        ScenarioGap::DestinationZeroRate => "a zero-rated line on an OSS sale",
                        ScenarioGap::DomesticZeroRate => "a line without VAT on a domestic sale",
                    };
                    format!(
                        "unsupported VAT scenario for event ord_1: {treatment}; v0.1 cannot \
                         post it, so book this order by hand"
                    )
                }
                PostingError::VatRateMismatch { .. } => "event ord_1, lines[1]: vat_rate \
                     `standard` does not fit a EuB2cOss sale; SeDomestic takes a Swedish \
                     class, EuB2cOss a destination percentage, EuB2bReverse and \
                     ExportOutsideEu `zero`"
                    .to_owned(),
                PostingError::UnlistedOssRate { .. } => "event ord_1, lines[0]: 19.00 % is \
                     not listed for `DE` in [vat.rates.oss]; correct the line, or add the \
                     rate if the checkout charged it"
                    .to_owned(),
            };
            assert_eq!(err.to_string(), expected);
            assert!(err.source().is_none());
        }
    }
}
