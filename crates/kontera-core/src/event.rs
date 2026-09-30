//! The four input events. See docs/02 §5 and docs/04 §2.
//!
//! This is the entire input vocabulary of the library, and it is the contract
//! with a seven-year retention obligation attached (ADR-0005): a v1 event
//! written in 2026 must still parse in 2033.
//!
//! Every type here is a boundary parser (docs/08 §2). Strings are checked for
//! shape and nothing more — a [`CountryCode`] is two capital letters, not a
//! membership test against an ISO list, and a [`VatNumber`] is non-empty, not
//! VIES-validated. Both of those are the host's job (docs/04 §2.1); what the
//! crate proves is that the value is not garbage.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The version of the event schema this crate reads and writes.
///
/// Independent of the crate version (ADR-0005). Bumped only on a breaking
/// change to the wire form; the deserialiser then supports this version and
/// the one before it.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Why a value could not become one of the event types.
///
/// Nests into `PostingError` via `#[from]`, one enum per module (docs/08 §4).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EventError {
    /// An event id must be non-empty.
    #[error("`{input}` is not an event id; expected a non-empty string, e.g. \"ord_1001\"")]
    InvalidEventId {
        /// The rejected input, verbatim.
        input: String,
    },
    /// A provider id must be non-empty ASCII lowercase letters, digits, `_`
    /// or `-`.
    #[error("`{input}` is not a provider id; expected lowercase letters, digits, `_` or `-`, e.g. \"stripe\"")]
    InvalidProviderId {
        /// The rejected input, verbatim.
        input: String,
    },
    /// A country code must be exactly two ASCII capital letters.
    #[error("`{input}` is not a country code; expected two capital letters, e.g. \"SE\"")]
    InvalidCountryCode {
        /// The rejected input, verbatim.
        input: String,
    },
    /// A VAT number must be non-empty.
    #[error("`{input}` is not a VAT number; expected a non-empty string, e.g. \"SE556000000001\"")]
    InvalidVatNumber {
        /// The rejected input, verbatim.
        input: String,
    },
}

/// Host-assigned identity of one event: globally unique and stable
/// (docs/04 §2).
///
/// The only shape requirement is non-empty. An empty id would still be
/// "unique" the first time, but it is useless in an error message and as a
/// `covers` reference, so it fails at the boundary. Ordering is lexical:
/// `(date, event_id)` is the sort key before numbering (docs/03 §4.2).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EventId(String);

impl EventId {
    /// Parse the wire form: any non-empty string.
    ///
    /// # Errors
    ///
    /// [`EventError::InvalidEventId`] if the string is empty.
    pub fn parse(s: &str) -> Result<EventId, EventError> {
        s.parse()
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for EventId {
    type Err = EventError;

    fn from_str(s: &str) -> Result<EventId, EventError> {
        if s.is_empty() {
            return Err(EventError::InvalidEventId {
                input: s.to_owned(),
            });
        }
        Ok(EventId(s.to_owned()))
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl Serialize for EventId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Accepts only a non-empty JSON string.
impl<'de> Deserialize<'de> for EventId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<EventId, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A payment provider, as the host names it: `"stripe"`, `"klarna"`.
///
/// This is a lookup key into `[accounts.receivable]` in the config
/// (docs/04 §3), so it is restricted to the characters of a TOML bare key in
/// lowercase — `a-z`, `0-9`, `_`, `-`. Case is the one way an event and a
/// config key can disagree while looking identical, so only one case is
/// admitted.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProviderId(String);

impl ProviderId {
    /// Parse the wire form: `"stripe"`.
    ///
    /// # Errors
    ///
    /// [`EventError::InvalidProviderId`] if the string is empty or contains
    /// anything but ASCII lowercase letters, digits, `_` or `-`.
    pub fn parse(s: &str) -> Result<ProviderId, EventError> {
        s.parse()
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProviderId {
    type Err = EventError;

    fn from_str(s: &str) -> Result<ProviderId, EventError> {
        let bare_key_char =
            |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-';
        if s.is_empty() || !s.bytes().all(bare_key_char) {
            return Err(EventError::InvalidProviderId {
                input: s.to_owned(),
            });
        }
        Ok(ProviderId(s.to_owned()))
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl Serialize for ProviderId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Accepts only a JSON string in the [`ProviderId::parse`] grammar.
impl<'de> Deserialize<'de> for ProviderId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ProviderId, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// An ISO 3166-1 alpha-2 country code: `SE`, `DE`, `NO`.
///
/// Proven to be two ASCII capital letters, and nothing more. Whether the code
/// is assigned, and whether the country is in the EU, are questions for
/// `vat.rs` against a sourced table (docs/02 §3, **VERIFY**) — not for a
/// parser. `Copy`, because it is two bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CountryCode([u8; 2]);

impl CountryCode {
    /// Sweden. The one code the domestic scenario is defined by.
    pub const SE: CountryCode = CountryCode(*b"SE");

    /// Parse the wire form: `"SE"`.
    ///
    /// # Errors
    ///
    /// [`EventError::InvalidCountryCode`] unless the text is exactly two ASCII
    /// capital letters.
    pub fn parse(s: &str) -> Result<CountryCode, EventError> {
        s.parse()
    }
}

impl FromStr for CountryCode {
    type Err = EventError;

    fn from_str(s: &str) -> Result<CountryCode, EventError> {
        // Two bytes that are both ASCII capitals. A multi-byte character such
        // as `Å` is two or more bytes and falls through to the error arm.
        match s.as_bytes() {
            [a, b] if a.is_ascii_uppercase() && b.is_ascii_uppercase() => Ok(CountryCode([*a, *b])),
            [] | [_] | [_, _] | [_, _, _, ..] => Err(EventError::InvalidCountryCode {
                input: s.to_owned(),
            }),
        }
    }
}

/// The two letters: `SE`.
impl fmt::Display for CountryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", char::from(self.0[0]), char::from(self.0[1]))
    }
}

impl Serialize for CountryCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Accepts only a JSON string in the [`CountryCode::parse`] grammar.
impl<'de> Deserialize<'de> for CountryCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<CountryCode, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A buyer's VAT registration number, as the host asserts it.
///
/// Not validated against VIES — that is network I/O, which the host does
/// before emitting the event (docs/04 §2.1). The crate only proves it is
/// non-empty, because a business buyer with an empty VAT number is exactly
/// the input that would otherwise slide into reverse charge unnoticed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VatNumber(String);

impl VatNumber {
    /// Parse the wire form: any non-empty string.
    ///
    /// # Errors
    ///
    /// [`EventError::InvalidVatNumber`] if the string is empty.
    pub fn parse(s: &str) -> Result<VatNumber, EventError> {
        s.parse()
    }

    /// The number as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for VatNumber {
    type Err = EventError;

    fn from_str(s: &str) -> Result<VatNumber, EventError> {
        if s.is_empty() {
            return Err(EventError::InvalidVatNumber {
                input: s.to_owned(),
            });
        }
        Ok(VatNumber(s.to_owned()))
    }
}

impl fmt::Display for VatNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl Serialize for VatNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Accepts only a non-empty JSON string.
impl<'de> Deserialize<'de> for VatNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<VatNumber, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// The Swedish VAT rate a sale line is subject to (docs/02 §3).
///
/// A *classification*, not a number: the percentage each variant maps to is
/// supplied by the host in `[vat.rates]` (docs/04 §3) and applied in
/// `vat.rs`. Hardcoding `25` here would put regulated data in a type.
/// Wire form is `snake_case`: `"standard"`, `"reduced12"`, `"reduced6"`,
/// `"zero"` — the same spellings as the config keys.
///
/// Never add a wildcard arm over this enum (docs/08 §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VatRate {
    /// The standard rate — 25 % in Sweden.
    Standard,
    /// The first reduced rate — 12 % in Sweden (food, hotels).
    Reduced12,
    /// The second reduced rate — 6 % in Sweden (books, passenger transport).
    Reduced6,
    /// Zero-rated. Distinct from exempt, which is out of scope (docs/02 §3).
    Zero,
}

/// Whether a sale line is goods or a service. Affects EU treatment
/// (docs/04 §2). Wire form: `"goods"`, `"services"`.
///
/// Never add a wildcard arm over this enum (docs/08 §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupplyKind {
    /// Physical goods.
    Goods,
    /// Services.
    Services,
}

/// What a payment provider charged a fee for (docs/04 §2). Wire form:
/// `"transaction"`, `"payout"`, `"dispute"`, `"other"`.
///
/// v0.1 posts all four to the same expense account (docs/02 §4.3); the
/// distinction is recorded so a host can report on it and so a later
/// per-kind mapping is an additive config change.
///
/// Never add a wildcard arm over this enum (docs/08 §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeeKind {
    /// A per-transaction processing fee.
    Transaction,
    /// A fee for the payout itself.
    Payout,
    /// A chargeback or dispute fee.
    Dispute,
    /// Anything else the provider charged.
    Other,
}

#[cfg(test)]
mod tests {
    // Tests are the one place `unwrap()` is right: a wrong `Err` here is a test
    // failure, which is the point. The crate-level deny still holds for library code.
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn event_id_is_any_non_empty_string() {
        assert_eq!(EventId::parse("ord_1001").unwrap().as_str(), "ord_1001");
        assert_eq!(EventId::parse("ö").unwrap().to_string(), "ö");
        assert_eq!(
            EventId::parse(""),
            Err(EventError::InvalidEventId {
                input: String::new()
            })
        );
        assert!(EventId::parse("a").unwrap() < EventId::parse("b").unwrap());
    }

    #[test]
    fn provider_id_is_a_lowercase_bare_key() {
        for s in ["stripe", "klarna", "swish-2", "pay_pal", "x", "0"] {
            assert_eq!(ProviderId::parse(s).unwrap().as_str(), s, "{s:?}");
        }
        for s in [
            "",
            "Stripe",
            "stripe ",
            "str ipe",
            "stripe.com",
            "stripé",
            "STRIPE",
        ] {
            assert_eq!(
                ProviderId::parse(s),
                Err(EventError::InvalidProviderId {
                    input: s.to_owned()
                }),
                "{s:?}"
            );
        }
    }

    #[test]
    fn country_code_is_two_capitals() {
        assert_eq!(CountryCode::parse("SE").unwrap(), CountryCode::SE);
        assert_eq!(CountryCode::parse("DE").unwrap().to_string(), "DE");
        assert!(CountryCode::parse("DE").unwrap() < CountryCode::SE);
        for s in ["", "S", "se", "Se", "SWE", "S1", "ÅL", "SE "] {
            assert_eq!(
                CountryCode::parse(s),
                Err(EventError::InvalidCountryCode {
                    input: s.to_owned()
                }),
                "{s:?}"
            );
        }
    }

    #[test]
    fn vat_number_is_any_non_empty_string() {
        assert_eq!(
            VatNumber::parse("SE556000000001").unwrap().as_str(),
            "SE556000000001"
        );
        assert_eq!(
            VatNumber::parse(""),
            Err(EventError::InvalidVatNumber {
                input: String::new()
            })
        );
    }

    #[test]
    fn error_messages_name_the_input() {
        assert_eq!(
            CountryCode::parse("swe").unwrap_err().to_string(),
            "`swe` is not a country code; expected two capital letters, e.g. \"SE\""
        );
        assert_eq!(
            ProviderId::parse("Stripe").unwrap_err().to_string(),
            "`Stripe` is not a provider id; expected lowercase letters, digits, `_` or `-`, e.g. \"stripe\""
        );
    }

    #[test]
    fn identifier_serde_is_string_only() {
        assert_eq!(
            serde_json::to_string(&EventId::parse("ord_1001").unwrap()).unwrap(),
            r#""ord_1001""#
        );
        assert_eq!(
            serde_json::from_str::<EventId>(r#""ord_1001""#).unwrap(),
            EventId::parse("ord_1001").unwrap()
        );
        assert_eq!(
            serde_json::from_str::<CountryCode>(r#""SE""#).unwrap(),
            CountryCode::SE
        );
        assert_eq!(
            serde_json::to_string(&ProviderId::parse("stripe").unwrap()).unwrap(),
            r#""stripe""#
        );
        assert!(serde_json::from_str::<EventId>(r#""""#).is_err());
        assert!(serde_json::from_str::<EventId>("1001").is_err());
        assert!(serde_json::from_str::<ProviderId>(r#""Stripe""#).is_err());
        assert!(serde_json::from_str::<CountryCode>(r#""se""#).is_err());
        assert!(serde_json::from_str::<VatNumber>(r#""""#).is_err());
    }

    #[test]
    fn enum_wire_forms_match_the_config_keys() {
        // docs/04 §3 spells the config keys `standard`, `reduced12`,
        // `reduced6`; the event form must be the same word.
        assert_eq!(
            serde_json::to_string(&VatRate::Standard).unwrap(),
            r#""standard""#
        );
        assert_eq!(
            serde_json::to_string(&VatRate::Reduced12).unwrap(),
            r#""reduced12""#
        );
        assert_eq!(
            serde_json::to_string(&VatRate::Reduced6).unwrap(),
            r#""reduced6""#
        );
        assert_eq!(serde_json::to_string(&VatRate::Zero).unwrap(), r#""zero""#);
        assert_eq!(
            serde_json::to_string(&SupplyKind::Goods).unwrap(),
            r#""goods""#
        );
        assert_eq!(
            serde_json::to_string(&SupplyKind::Services).unwrap(),
            r#""services""#
        );
        assert_eq!(
            serde_json::to_string(&FeeKind::Transaction).unwrap(),
            r#""transaction""#
        );
        assert_eq!(
            serde_json::to_string(&FeeKind::Dispute).unwrap(),
            r#""dispute""#
        );
        assert_eq!(
            serde_json::from_str::<VatRate>(r#""reduced12""#).unwrap(),
            VatRate::Reduced12
        );
        assert!(serde_json::from_str::<VatRate>(r#""Standard""#).is_err());
        assert!(serde_json::from_str::<VatRate>(r#""25""#).is_err());
        assert!(serde_json::from_str::<VatRate>("25").is_err());
    }
}
