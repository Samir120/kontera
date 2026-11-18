//! VAT scenario determination. See docs/02 §3.
//!
//! Exactly four scenarios in v0.1. Anything else is `Err`, never a guess
//! (ADR-0004). [`classify`] picks the scenario of a sale from its buyer and
//! its `ship_to`. Whether each line of the sale fits that scenario is checked
//! separately (ADR-0006 §1).
//!
//! [`VatPercent`] lives here rather than in `config.rs` because [`line_vat`],
//! the `rate × net` multiply that consumes it, belongs here. `Money::rounded`
//! stays the crate's single rounding site.

use std::fmt;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::SeRates;
use crate::error::PostingError;
use crate::event::{BuyerTaxStatus, CountryCode, SaleCaptured, SaleLine, VatRate};
use crate::money::{Money, MoneyError};

/// Why a value could not become one of the VAT types.
///
/// Not nested into `PostingError`: a [`VatPercent`] is proven when the config
/// is deserialised, before `post()` is called, so no path through `post()`
/// can produce this error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VatError {
    /// A VAT percentage must be `0`–`99` with at most two decimals, no
    /// leading zero, and greater than zero.
    #[error("`{input}` is not a VAT percentage; expected a string such as \"25\" or \"12.00\", greater than 0 and less than 100")]
    InvalidPercent {
        /// The rejected input, verbatim.
        input: String,
    },
}

/// A VAT rate as a percentage, as the host configures it: `"25.00"`.
///
/// Grammar: an integer part of `0` or `1`–`99` with no leading zero,
/// optionally followed by `.` and one or two digits. The value must be
/// greater than zero. The grammar alone keeps it below 100, so no input can
/// overflow a `Decimal`. No sign, exponent or whitespace.
///
/// Zero is rejected (ADR-0006 §2). Zero has one spelling, the `VatRate::Zero`
/// class, and a destination zero rate is not an OSS sale at 0 %: it is a
/// different treatment that v0.1 refuses.
///
/// Stored at scale 2, so `"25"` and `"25.00"` are one value and display the
/// same. Serialises as that string, and deserialises from a string only: a
/// TOML or JSON number is a float in most parsers (ADR-0002).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VatPercent(Decimal);

impl VatPercent {
    /// Parse the wire form, e.g. `"25.00"`.
    ///
    /// # Errors
    ///
    /// [`VatError::InvalidPercent`] if the string breaks the grammar above
    /// or is zero.
    pub fn parse(s: &str) -> Result<VatPercent, VatError> {
        s.parse()
    }
}

impl FromStr for VatPercent {
    type Err = VatError;

    fn from_str(s: &str) -> Result<VatPercent, VatError> {
        let invalid = || VatError::InvalidPercent {
            input: s.to_owned(),
        };
        let (int, frac) = match s.split_once('.') {
            Some((int, frac)) => (int, Some(frac)),
            None => (s, None),
        };
        let int_ok = matches!(int.as_bytes(), [b'0'..=b'9'] | [b'1'..=b'9', b'0'..=b'9']);
        let frac_ok =
            frac.is_none_or(|f| matches!(f.as_bytes(), [b'0'..=b'9'] | [b'0'..=b'9', b'0'..=b'9']));
        if !(int_ok && frac_ok) {
            return Err(invalid());
        }
        let mut value = Decimal::from_str(s).map_err(|_| invalid())?;
        if value <= Decimal::ZERO {
            return Err(invalid());
        }
        value.rescale(2);
        Ok(VatPercent(value))
    }
}

/// The value itself: `"25.00"` becomes `25.00`, not `0.25`.
impl From<VatPercent> for Decimal {
    fn from(p: VatPercent) -> Decimal {
        p.0
    }
}

impl fmt::Display for VatPercent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl Serialize for VatPercent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Accepts only a string. `25` and `25.0` as numbers are errors.
impl<'de> Deserialize<'de> for VatPercent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<VatPercent, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// The VAT on one line: `net × percent / 100`, rounded half away from zero.
///
/// Apply it per line and then sum the lines. Rounding a sum gives a different
/// number from the one the checkout charged (docs/02 §2.1).
///
/// A Swedish class takes its percentage from `se`, the `[vat.rates.se]` table.
/// [`VatRate::Destination`] uses the percentage on the line. [`VatRate::Zero`]
/// is no VAT, and has no percentage because [`VatPercent`] can't be zero
/// (ADR-0006 §2).
///
/// This is arithmetic only. It does not check that the rate is valid for the
/// sale's scenario.
///
/// # Errors
///
/// None in practice. A percentage is below 100, so the result is never larger
/// than the net, and the net is a [`Money`], which is already in range. The
/// `Result` is [`Money::rounded`]'s, and `tests/vat.rs` runs every percentage
/// at the largest net.
pub fn line_vat(line: &SaleLine, se: &SeRates<VatPercent>) -> Result<Money, MoneyError> {
    let percent = match line.vat_rate {
        VatRate::Standard => se.standard,
        VatRate::Reduced12 => se.reduced12,
        VatRate::Reduced6 => se.reduced6,
        VatRate::Destination(percent) => percent,
        VatRate::Zero => return Ok(Money::ZERO),
    };
    let net = Decimal::from(line.amount_excl_vat());
    Money::rounded(net * percent.0 / Decimal::ONE_HUNDRED)
}

// ── Territory tables ─────────────────────────────────────────────────────
//
// Two sourced tables. Each is text, so it can be read against its source.
// Each entry goes through the boundary parser, so a malformed entry matches
// nothing, and `tests/vat.rs` counts the codes each table accepts.

/// The EU VAT area as `ship_to` codes: the 27 member states, Monaco and
/// Northern Ireland.
///
/// Source: mervärdesskattelagen (2023:200) 2 kap. 21–23 §§, which carry out
/// Articles 5–7 of Directive 2006/112/EC.
///
/// - Greece is `GR`. `EL` is its VAT number prefix and not a country code.
/// - `MC`: 21 § equates Monaco with an EU country, and 23 § treats a sale to
///   Monaco as a sale to France.
/// - `XI`: Northern Ireland, for goods only. ISO never assigned the code. The
///   EU uses it for Northern Ireland, and `GB` stays outside the VAT area.
/// - The Isle of Man (`IM`) is left out. 21 § equates it with an EU country,
///   but 23 § treats a sale to it as a sale to the United Kingdom.
/// - A territory that 22 § excludes and that has its own code, such as Åland
///   (`AX`), is left out. One without its own code, such as the Canary
///   Islands, can't be told from its member state by a country code. The host
///   must not send the member state's code for it.
// VERIFY (Q13): read against ML (2023:200) 2 kap. 21–23 §§ on riksdagen.se
// (t.o.m. SFS 2026:1025) on 2026-11-17, and against Skatteverket's "Sälja
// varor till länder utanför EU" (modified 2026-08-24) for Åland and Northern
// Ireland. Still to confirm: the Directive's current Articles 5–7 on EUR-Lex,
// the Isle of Man reading, the legal basis and end date of Northern Ireland's
// status for goods, and Skatteverket's "Territoriella avgränsningar".
const EU_VAT_AREA: [&str; 29] = [
    "AT", "BE", "BG", "CY", "CZ", "DE", "DK", "EE", "ES", "FI", "FR", "GR", "HR", "HU", "IE", "IT",
    "LT", "LU", "LV", "MC", "MT", "NL", "PL", "PT", "RO", "SE", "SI", "SK", "XI",
];

/// The officially assigned ISO 3166-1 alpha-2 codes.
///
/// User-assigned and reserved codes are not here, even ones in common use:
/// `XK` (Kosovo), `XI`, `UK`, `EU`, `EL`, `IC` (the Canary Islands) and `EA`
/// (Ceuta and Melilla).
// VERIFY (Q14): taken from pycountry 26.2.16, which packages Debian's
// iso-codes, on 2026-11-17. Confirm against ISO's own list, the Online
// Browsing Platform at iso.org/obp, and recheck it when ISO assigns or
// withdraws a code.
const ISO_3166_1_ASSIGNED: [&str; 249] = [
    "AD", "AE", "AF", "AG", "AI", "AL", "AM", "AO", "AQ", "AR", "AS", "AT", "AU", "AW", "AX", "AZ",
    "BA", "BB", "BD", "BE", "BF", "BG", "BH", "BI", "BJ", "BL", "BM", "BN", "BO", "BQ", "BR", "BS",
    "BT", "BV", "BW", "BY", "BZ", "CA", "CC", "CD", "CF", "CG", "CH", "CI", "CK", "CL", "CM", "CN",
    "CO", "CR", "CU", "CV", "CW", "CX", "CY", "CZ", "DE", "DJ", "DK", "DM", "DO", "DZ", "EC", "EE",
    "EG", "EH", "ER", "ES", "ET", "FI", "FJ", "FK", "FM", "FO", "FR", "GA", "GB", "GD", "GE", "GF",
    "GG", "GH", "GI", "GL", "GM", "GN", "GP", "GQ", "GR", "GS", "GT", "GU", "GW", "GY", "HK", "HM",
    "HN", "HR", "HT", "HU", "ID", "IE", "IL", "IM", "IN", "IO", "IQ", "IR", "IS", "IT", "JE", "JM",
    "JO", "JP", "KE", "KG", "KH", "KI", "KM", "KN", "KP", "KR", "KW", "KY", "KZ", "LA", "LB", "LC",
    "LI", "LK", "LR", "LS", "LT", "LU", "LV", "LY", "MA", "MC", "MD", "ME", "MF", "MG", "MH", "MK",
    "ML", "MM", "MN", "MO", "MP", "MQ", "MR", "MS", "MT", "MU", "MV", "MW", "MX", "MY", "MZ", "NA",
    "NC", "NE", "NF", "NG", "NI", "NL", "NO", "NP", "NR", "NU", "NZ", "OM", "PA", "PE", "PF", "PG",
    "PH", "PK", "PL", "PM", "PN", "PR", "PS", "PT", "PW", "PY", "QA", "RE", "RO", "RS", "RU", "RW",
    "SA", "SB", "SC", "SD", "SE", "SG", "SH", "SI", "SJ", "SK", "SL", "SM", "SN", "SO", "SR", "SS",
    "ST", "SV", "SX", "SY", "SZ", "TC", "TD", "TF", "TG", "TH", "TJ", "TK", "TL", "TM", "TN", "TO",
    "TR", "TT", "TV", "TW", "TZ", "UA", "UG", "UM", "US", "UY", "UZ", "VA", "VC", "VE", "VG", "VI",
    "VN", "VU", "WF", "WS", "YE", "YT", "ZA", "ZM", "ZW",
];

fn listed(table: &[&str], code: CountryCode) -> bool {
    table
        .iter()
        .any(|entry| CountryCode::parse(entry).is_ok_and(|listed| listed == code))
}

/// Whether `code` is in the EU VAT area, where EU VAT rules apply to a sale of
/// goods.
///
/// This is not EU membership. Åland (`AX`) belongs to a member state and is
/// outside. Monaco (`MC`) and Northern Ireland (`XI`) belong to none and are
/// inside. `GB` is outside, so a host sends `XI` for Northern Ireland.
///
/// A country code can't separate the Canary Islands from Spain. `ship_to` must
/// name the VAT territory, and the host is the one who knows it (docs/04 §2.1).
#[must_use]
pub fn in_eu_vat_area(code: CountryCode) -> bool {
    listed(&EU_VAT_AREA, code)
}

/// Whether `code` is an officially assigned ISO 3166-1 alpha-2 code.
///
/// [`CountryCode`] proves two capital letters and no more. This is the
/// positive check that a code names a country: `DD` and `XX` have the shape
/// and are not assigned. Neither are `XI` and `XK`, although both are in use.
#[must_use]
pub fn is_assigned_country_code(code: CountryCode) -> bool {
    listed(&ISO_3166_1_ASSIGNED, code)
}

// ── Scenario ─────────────────────────────────────────────────────────────

/// The VAT treatment of a sale: one of the four scenarios of docs/02 §3.
///
/// Never add a wildcard arm over this enum (docs/08 §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VatScenario {
    /// A sale within Sweden, at a Swedish rate, to any buyer.
    SeDomestic,
    /// Goods to a business elsewhere in the EU VAT area. No Swedish VAT is
    /// charged, and the buyer accounts for the VAT.
    EuB2bReverse,
    /// Goods to a consumer elsewhere in the EU VAT area, at the destination's
    /// rate, reported through OSS.
    EuB2cOss,
    /// A sale shipped outside the EU VAT area. No VAT, whoever the buyer is.
    ExportOutsideEu,
}

/// The scenario of `sale`, from its `ship_to` and its buyer.
///
/// The questions are asked in this order, and the first yes decides:
///
/// 1. `SE` is [`VatScenario::SeDomestic`], whoever the buyer is. Sweden is in
///    the VAT area, so this has to come first.
/// 2. Anywhere else in the EU VAT area goes by the buyer: a business is
///    [`VatScenario::EuB2bReverse`] and a consumer is [`VatScenario::EuB2cOss`].
/// 3. An assigned ISO 3166-1 code outside the VAT area is
///    [`VatScenario::ExportOutsideEu`].
///
/// A code that passes none of them is an error. Export means no VAT, so a
/// code that names no country must never fall through to it.
///
/// The lines are not read. A line can still be wrong for the scenario
/// returned here, and that check is separate (ADR-0006 §1). The host asserts
/// the buyer's status and VAT number (docs/04 §2.1), and nothing here
/// questions either.
///
/// # Errors
///
/// [`PostingError::UnknownShipTo`] if `ship_to` is neither in the EU VAT area
/// nor an assigned ISO 3166-1 code. `XK`, `IC` and `EA` are such codes,
/// although platforms send them, and so are `UK` and `EL`.
// See docs/02 §3 and §3.1. VERIFY (Q13, Q14): the two tables above decide
// every answer here, and neither is confirmed yet.
pub fn classify(sale: &SaleCaptured) -> Result<VatScenario, PostingError> {
    let ship_to = sale.ship_to;
    if ship_to == CountryCode::SE {
        return Ok(VatScenario::SeDomestic);
    }
    if in_eu_vat_area(ship_to) {
        return Ok(match sale.buyer {
            BuyerTaxStatus::Business { .. } => VatScenario::EuB2bReverse,
            BuyerTaxStatus::Consumer => VatScenario::EuB2cOss,
        });
    }
    if is_assigned_country_code(ship_to) {
        return Ok(VatScenario::ExportOutsideEu);
    }
    Err(PostingError::UnknownShipTo {
        event: sale.id.clone(),
        ship_to,
    })
}
