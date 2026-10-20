//! VAT scenario determination. See docs/02 §3.
//!
//! Exactly four scenarios in v0.1. Anything else is `Err`, never a guess
//! (ADR-0004). Scenario determination lands in T9.
//!
//! [`VatPercent`] lives here rather than in `config.rs` because the
//! `rate × net` multiply that consumes it belongs here, next to
//! `Money::rounded`, the crate's single rounding site.

use std::fmt;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};

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
/// Zero is rejected for now. Every rate v0.1 configures is a positive Swedish
/// rate, and zero-rating is the `VatRate::Zero` class, not a configured
/// number. Whether a destination country's zero rate is a `VatPercent` is
/// ADR-0006's call (T13).
///
/// Stored at scale 2, so `"25"` and `"25.00"` are one value and display the
/// same. Deserialises from a string only: a TOML or JSON number is a float in
/// most parsers (ADR-0002).
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

impl fmt::Display for VatPercent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
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
