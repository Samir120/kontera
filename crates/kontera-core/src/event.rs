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
//!
//! Two amounts carry a sign constraint — a sale line is never negative, a fee
//! is strictly positive — and each is the one private field of its struct, so
//! the constructor is the only way in and a JSON goes through it too.

use std::fmt;
use std::str::FromStr;

use chrono::NaiveDate;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::money::Money;

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
    /// A sale line's net amount must not be negative. A negative line is a
    /// refund, and refund are their own event.
    #[error("sale line `{description}` has negative amount {amount}; a refund is its own event")]
    NegativeLineAmount {
        /// The line's description, for locating it.
        description: String,
        /// The rejected amount.
        amount: Money,
    },
    /// A fee must be strictly positive. A provider crediting a fee back is a
    /// scenario v0.1 does not model.
    #[error("fee {event} has amount {amount}; a fee must be positive")]
    NonPositiveFee {
        /// The fee event's id.
        event: EventId,
        /// The rejected amount.
        amount: Money,
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

/// How the buyer is treated for VAT (docs/04 §2).
///
/// Wire form is tagged `kind`: `{"kind":"consumer"}` or
/// `{"kind":"business", "vat_number":"SE556000000001"}`. The host asserts the
/// status; the crate records it.
///
/// Never add a wildcard arm over this enum (docs/08 §6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuyerTaxStatus {
    /// A private individual. Destination-rate VAT applies within the EU.
    Consumer,
    /// A VAT-registered business. Reverse charge applies withing the EU.
    Business {
        /// The buyer's VAT number, as validity by the host.
        vat_number: VatNumber,
    },
}

/// One line of a sale or refund (docs/04 §2).
///
/// `amount_excl_vat` is the net figure the host's checkout computed, never
/// backed out of a gross by division (docs/04 §2.1), and it is not negative:
/// a negative line is a refund, and refund are their own event. That one
/// field is private so [`SaleLine::new`] is the only way in; the other three
/// are proven types and stay public.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SaleLineWire")]
pub struct SaleLine {
    /// What was sold. Free text; may reach teh SIE output.
    pub description: String,
    amount_excl_vat: Money,
    /// The rate this line is taxed at.
    pub vat_rate: VatRate,
    /// Goods or services — decides EU treatment.
    pub goods_or_services: SupplyKind,
}

impl SaleLine {
    /// Build a line, refusing a negative amount. Zero is allowed: a
    /// complimentary line is still a line.
    ///
    /// # Errors
    ///
    /// [`EventError::NegativeLineAmount`] if `amount_excl_vat` is below zero.
    pub fn new(
        description: String,
        amount_excl_vat: Money,
        vat_rate: VatRate,
        goods_or_services: SupplyKind,
    ) -> Result<SaleLine, EventError> {
        if amount_excl_vat.is_negative() {
            return Err(EventError::NegativeLineAmount {
                description,
                amount: amount_excl_vat,
            });
        }
        Ok(SaleLine {
            description,
            amount_excl_vat,
            vat_rate,
            goods_or_services,
        })
    }

    /// The wire shape of [`SaleLine`], before the sign check.
    #[must_use]
    pub const fn amount_excl_vat(&self) -> Money {
        self.amount_excl_vat
    }
}

/// The wire shape of [`SaleLine`], before the sign check.
#[derive(Deserialize)]
struct SaleLineWire {
    description: String,
    amount_excl_vat: Money,
    vat_rate: VatRate,
    goods_or_services: SupplyKind,
}

impl TryFrom<SaleLineWire> for SaleLine {
    type Error = EventError;

    fn try_from(w: SaleLineWire) -> Result<SaleLine, EventError> {
        SaleLine::new(
            w.description,
            w.amount_excl_vat,
            w.vat_rate,
            w.goods_or_services,
        )
    }
}

/// A customer's payment was captured (docs/02 §5). The accounting event for
/// a sale — an order that is never paid produces no bookkeeping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaleCaptured {
    /// Host-assigned, unique across the log.
    pub id: EventId,
    /// Capture date — the verification date.
    pub at: NaiveDate,
    /// Who collected the money.
    pub provider: ProviderId,
    /// Consumer or business.
    pub buyer: BuyerTaxStatus,
    /// Description country, ISO 3166-1 alpha-2. Decides the VAT scenario.
    pub ship_to: CountryCode,
    /// What was sold, net of VAT.
    pub lines: Vec<SaleLine>,
}

/// Money was returned to a customer (docs/02 §5). Carrier lines, not a
/// total: a partial refund of mixed-VAT order cannot be apportioned from a
/// total alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefundIssued {
    /// Host-assigned, unique across the log.
    pub id: EventId,
    /// Refund date.
    pub at: NaiveDate,
    /// The sale being reversed, in whole or in part.
    pub original_sale: EventId,
    /// What was refunded, net of VAT, as positive amounts.
    pub lines: Vec<SaleLine>,
}

/// A payment provider charged a fee (docs/02 §5).
///
/// `amount` is strictly positive and is the one private field, so
/// [`FeeCharged::new`] is the only way in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeeChargedWire")]
pub struct FeeCharged {
    /// Host-assigned, unique across the log.
    pub id: EventId,
    /// Charge date.
    pub at: NaiveDate,
    /// Who charged it.
    pub provider: ProviderId,
    /// What it was for.
    pub kind: FeeKind,
    amount: Money,
}

impl FeeCharged {
    /// Build a fee, refusing a zero or negative amount.
    ///
    /// # Errors
    ///
    /// [`EventError::NonPositiveFee`] unless `amount` is above zero.
    pub fn new(
        id: EventId,
        at: NaiveDate,
        provider: ProviderId,
        kind: FeeKind,
        amount: Money,
    ) -> Result<FeeCharged, EventError> {
        if amount.is_zero() || amount.is_negative() {
            return Err(EventError::NonPositiveFee { event: id, amount });
        }
        Ok(FeeCharged {
            id,
            at,
            provider,
            kind,
            amount,
        })
    }

    /// The fee, proven positive.
    #[must_use]
    pub const fn amount(&self) -> Money {
        self.amount
    }
}

/// The wire shape of [`FeeCharged`], before the sign check.
#[derive(Deserialize)]
struct FeeChargedWire {
    id: EventId,
    at: NaiveDate,
    provider: ProviderId,
    kind: FeeKind,
    amount: Money,
}

impl TryFrom<FeeChargedWire> for FeeCharged {
    type Error = EventError;

    fn try_from(w: FeeChargedWire) -> Result<FeeCharged, EventError> {
        FeeCharged::new(w.id, w.at, w.provider, w.kind, w.amount)
    }
}

/// A provider transferred a net amount to the bank (docs/02 §5).
///
/// `covers` is explicit: the host has the provider's settlement report and
/// says which event this payout nets together. The library never guesses
/// (docs/04 §2.1). `amount` is unconstructed — a provider can net to a
/// negative payout — and whether it reconciles is invariant I2, checked by
/// `settle`, not by the type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayoutSettled {
    /// Host-assigned, unique across the log.
    pub id: EventId,
    /// The date the money landed.
    pub at: NaiveDate,
    /// Who paid out.
    pub provider: ProviderId,
    /// Net amount, as it hits the bank.
    pub amount: Money,
    /// The sale, refund and fee events this payout nets together.
    pub covers: Vec<EventId>,
}

/// One event in the log — the entire input vocabulary (docs/02 §5).
///
/// Wire form is one JSON object per line, tagged, `type` in `snake_case`:
/// `sale_captured`, `refund_issued`, `fee_charged`, `payout_settled`. Each
/// variant wraps its own struct so a rule can take a `&SaleCaptured` rather
/// than re-matching the enum.
///
/// Never add wildcard arm over this enum (docs/08 §6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// See [`SaleCaptured`].
    SaleCaptured(SaleCaptured),
    /// See [`RefundIssued`].
    RefundIssued(RefundIssued),
    /// See [`FeeCharged`].
    FeeCharged(FeeCharged),
    /// See [`PayoutSettled`].
    PayoutSettled(PayoutSettled),
}

impl Event {
    /// The event's id, whichever kind it is.
    #[must_use]
    pub fn id(&self) -> &EventId {
        match self {
            Event::SaleCaptured(e) => &e.id,
            Event::RefundIssued(e) => &e.id,
            Event::FeeCharged(e) => &e.id,
            Event::PayoutSettled(e) => &e.id,
        }
    }

    /// The event's date, whichever kind it is. With [`Event::id`], the sort
    /// key before numbering (docs/03 §4.2).
    #[must_use]
    pub fn at(&self) -> NaiveDate {
        match self {
            Event::SaleCaptured(e) => e.at,
            Event::RefundIssued(e) => e.at,
            Event::FeeCharged(e) => e.at,
            Event::PayoutSettled(e) => e.at,
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests are the one place `unwrap()` and `panic!()` are right: a wrong
    // `Err` or a wrong variant here is a test failure, which is the point.
    // The crate-level deny still holds for library code.
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn kr(s: &str) -> Money {
        Money::parse(s).unwrap()
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

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

    #[test]
    fn docs_04_example_lines_round_trip_byte_for_byte() {
        // The three JSONL lines from docs/04 §2.2, verbatim. Field order in
        // the structure matches the document, so serilaising back must give
        // the same bytes — that ties the code to the contract.
        let sale = r#"{"type":"sale_captured","id":"ord_1001","at":"2026-01-05","provider":"stripe","buyer":{"kind":"consumer"},"ship_to":"SE","lines":[{"description":"Widget","amount_excl_vat":"1000.00","vat_rate":"standard","goods_or_services":"goods"}]}"#;
        let fee = r#"{"type":"fee_charged","id":"fee_2026_01_08","at":"2026-01-08","provider":"stripe","kind":"transaction","amount":"1168.00"}"#;
        let payout = r#"{"type":"payout_settled","id":"po_9001","at":"2026-01-08","provider":"stripe","amount":"47832.00","covers":["ord_1001","fee_2026_01_08"]}"#;

        for line in [sale, fee, payout] {
            let event: Event = serde_json::from_str(line).unwrap();
            assert_eq!(serde_json::to_string(&event).unwrap(), line);
        }

        let Event::SaleCaptured(s) = serde_json::from_str::<Event>(sale).unwrap() else {
            panic!("tag `sale_captured` must produce SaleCaptured");
        };
        assert_eq!(s.id, EventId::parse("ord_1001").unwrap());
        assert_eq!(s.at, date(2026, 1, 5));
        assert_eq!(s.buyer, BuyerTaxStatus::Consumer);
        assert_eq!(s.ship_to, CountryCode::SE);
        assert_eq!(s.lines.len(), 1);
        assert_eq!(s.lines[0].amount_excl_vat(), kr("1000.00"));
        assert_eq!(s.lines[0].vat_rate, VatRate::Standard);

        let Event::FeeCharged(f) = serde_json::from_str::<Event>(fee).unwrap() else {
            panic!("tag `fee_charged` must produce FeeCharged");
        };
        assert_eq!(f.kind, FeeKind::Transaction);
        assert_eq!(f.amount, kr("1168.00"));

        let Event::PayoutSettled(p) = serde_json::from_str::<Event>(payout).unwrap() else {
            panic!("tag `payout_settled` must produce PayoutSettled");
        };
        assert_eq!(p.amount, kr("47832.00"));
        assert_eq!(p.covers.len(), 2);
        assert_eq!(p.covers[1], EventId::parse("fee_2026_01_08").unwrap());
    }

    #[test]
    fn id_and_at_are_the_sort_key_for_every_kind() {
        let refund = r#"{"type":"refund_issued","id":"rf_1","at":"2026-01-06","original_sale":"ord_1001","lines":[]}"#;
        let event: Event = serde_json::from_str(refund).unwrap();
        assert_eq!(event.id(), &EventId::parse("rf_1").unwrap());
        assert_eq!(event.at(), date(2026, 1, 6));
        assert_eq!(serde_json::to_string(&event).unwrap(), refund);
    }

    #[test]
    fn sale_line_amount_is_never_negative() {
        let line = |amount: &str| {
            SaleLine::new(
                "Widget".to_owned(),
                kr(amount),
                VatRate::Standard,
                SupplyKind::Goods,
            )
        };
        assert_eq!(line("0.00").unwrap().amount_excl_vat(), Money::ZERO);
        assert_eq!(
            line("-0.01"),
            Err(EventError::NegativeLineAmount {
                description: "Widget".to_string(),
                amount: kr("-0.01"),
            })
        );
        let json = r#"{"description":"Widget","amount_excl_vat":"-1.00","vat_rate":"standard","goods_or_services":"goods"}"#;
        let err = serde_json::from_str::<SaleLine>(json).unwrap_err();
        assert!(err.to_string().contains("negative amount -1.0"), "{err}");
    }

    #[test]
    fn fee_amount_is_strictly_positive() {
        let fee = |amount: &str| {
            FeeCharged::new(
                EventId::parse("fee_1").unwrap(),
                date(2026, 1, 8),
                ProviderId::parse("stripe").unwrap(),
                FeeKind::Payout,
                kr(amount),
            )
        };
        assert_eq!(fee("0.01").unwrap().amount(), kr("0.01"));
        for amount in ["0.00", "-1.00"] {
            assert_eq!(
                fee(amount),
                Err(EventError::NonPositiveFee {
                    event: EventId::parse("fee_1").unwrap(),
                    amount: kr(amount),
                }),
                "{amount}"
            );
        }
        let json = r#"{"type":"fee_charged","id":"fee_1","at":"2026-01-08","provider":"stripe","kind":"other","amount":"0.00"}"#;
        assert!(serde_json::from_str::<Event>(json).is_err());
    }

    #[test]
    fn business_buyer_carries_a_vat_number() {
        let business = r#"{"kind":"business","vat_number":"SE556000000001"}"#;
        let status: BuyerTaxStatus = serde_json::from_str(business).unwrap();
        assert_eq!(
            status,
            BuyerTaxStatus::Business {
                vat_number: VatNumber::parse("SE556000000001").unwrap()
            }
        );
        assert_eq!(serde_json::to_string(&status).unwrap(), business);
        assert!(serde_json::from_str::<BuyerTaxStatus>(r#"{"kind":"business"}"#).is_err());
        assert!(
            serde_json::from_str::<BuyerTaxStatus>(r#"{"kind":"business","vat_number":""}"#)
                .is_err()
        );
        assert!(serde_json::from_str::<BuyerTaxStatus>(r#"{"kind":"company"}"#).is_err());
    }

    #[test]
    fn malformed_event_fail_closed() {
        for json in [
            // unknown tag
            r#"{"type":"order_placed","id":"o_1","at":"2026-01-05"}"#,
            // missing tag
            r#"{"id":"po_1","at":"2026-01-08","provider":"stripe","amount":"1.00","covers":[]}"#,
            // JSON number where the schema says string (ADR-0002)
            r#"{"type":"fee_charged","id":"f_1","at":"2026-01-08","provider":"stripe","kind":"other","amount":1168.00}"#,
            // date not ISO
            r#"{"type":"payout_settled","id":"po_1","at":"08/01/2026","provider":"stripe","amount":"1.00","covers":[]}"#,
            // missing field
            r#"{"type":"sale_captured","id":"o_1","at":"2026-01-05","provider":"stripe","buyer":{"kind":"consumer"},"lines":[]}"#,
        ] {
            assert!(serde_json::from_str::<Event>(json).is_err(), "{json}");
        }
    }
}
