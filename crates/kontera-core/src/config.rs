//! Account mapping, VAT profile and period state. See docs/04 §3.
//!
//! Period open/closed is passed in, never remembered — the core has no memory
//! (ADR-0001). The host or CLI deserialises the config from TOML; `toml` is
//! not a core dependency (docs/03 §8). Every type here is a boundary parser,
//! so a config that deserialises is already proven.
//!
//! This module holds the leaf types. The `Config` tree lands in T8b.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Deserializer};

/// Why a value could not become one of the config types.
///
/// Not nested into `PostingError`: config is proven when it is deserialised,
/// before `post()` is called, so no path through `post()` can produce this.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The organisation number was empty.
    #[error("`org_number` is empty; give the company's organisation number")]
    EmptyOrgNumber,
    /// A month must be written `YYYY-MM`.
    #[error("`{input}` is not a month; expected \"YYYY-MM\", e.g. \"2026-01\"")]
    InvalidMonth {
        /// The rejected input, verbatim.
        input: String,
    },
    /// The fiscal year ends before it starts.
    #[error("fiscal year starts {start}, after it ends {end}; swap the dates")]
    FiscalYearReversed {
        /// The configured start.
        start: NaiveDate,
        /// The configured end.
        end: NaiveDate,
    },
    /// The fiscal year does not start on the 1st and end on the last day of
    /// a month.
    #[error("fiscal year {start} to {end} must start on the 1st of a month and end on the last day of a month")]
    FiscalYearNotMonthAligned {
        /// The configured start.
        start: NaiveDate,
        /// The configured end.
        end: NaiveDate,
    },
    /// A closed period lies outside the fiscal year.
    #[error("closed period {period} is outside fiscal year {start} to {end}; remove it")]
    ClosedPeriodOutsideFiscalYear {
        /// The offending period.
        period: Month,
        /// The fiscal year's start.
        start: NaiveDate,
        /// The fiscal year's end.
        end: NaiveDate,
    },
    /// A closed period is listed more than once.
    #[error("closed period {period} is listed more than once; remove the duplicate")]
    DuplicateClosedPeriod {
        /// The repeated period.
        period: Month,
    },
}

/// The company's organisation number, as the host supplies it.
///
/// Non-empty only. Its shape is checked in M3, when the SIE `#ORGNR` record
/// first writes it. Config examples use the placeholder `"XXXXXX-XXXX"`,
/// never a real number (docs/03 §5.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrgNumber(String);

impl OrgNumber {
    /// Parse the wire form: any non-empty string.
    ///
    /// # Errors
    ///
    /// [`ConfigError::EmptyOrgNumber`] if the string is empty.
    pub fn parse(s: &str) -> Result<OrgNumber, ConfigError> {
        s.parse()
    }

    /// The number as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for OrgNumber {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<OrgNumber, ConfigError> {
        if s.is_empty() {
            return Err(ConfigError::EmptyOrgNumber);
        }
        Ok(OrgNumber(s.to_owned()))
    }
}

impl fmt::Display for OrgNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Accepts only a non-empty string.
impl<'de> Deserialize<'de> for OrgNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<OrgNumber, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A calendar month, the unit of a closed period: `"2026-01"`.
///
/// A quarterly or annual filer lists every month of a filed period. Exactly
/// four ASCII digits, `-`, two ASCII digits, month `01`–`12`. Ordering is
/// chronological. Deserialises from a string only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Month {
    year: i32,  // field order is the Ord: year first
    month: u32, // 1–12
}

impl Month {
    /// Parse the wire form `YYYY-MM`.
    ///
    /// # Errors
    ///
    /// [`ConfigError::InvalidMonth`] if the string is not `YYYY-MM` with a
    /// month from `01` to `12`.
    pub fn parse(s: &str) -> Result<Month, ConfigError> {
        s.parse()
    }

    /// The month a date falls in. Crate-internal: every public `Month` comes
    /// from [`Month::parse`], so it displays as it parses.
    pub(crate) fn containing(date: NaiveDate) -> Month {
        Month {
            year: date.year(),
            month: date.month(),
        }
    }
}

impl FromStr for Month {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Month, ConfigError> {
        let invalid = || ConfigError::InvalidMonth {
            input: s.to_owned(),
        };
        let b = s.as_bytes();
        let shape =
            b.len() == 7 && b[4] == b'-' && b[..4].iter().chain(&b[5..]).all(u8::is_ascii_digit);
        if !shape {
            return Err(invalid());
        }
        let year = s[..4].parse().map_err(|_| invalid())?;
        let month = s[5..].parse().map_err(|_| invalid())?;
        if !(1..=12).contains(&month) {
            return Err(invalid());
        }
        Ok(Month { year, month })
    }
}

impl fmt::Display for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

/// Accepts only a `YYYY-MM` string.
impl<'de> Deserialize<'de> for Month {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Month, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// The räkenskapsår, and which of its months are closed (docs/02 §7).
///
/// Month-aligned: it starts on the 1st and ends on a month's last day.
/// Months are the closed-period unit, and BFL (1999:1078) 3 kap. 1 § has a
/// räkenskapsår cover whole calendar months. Length is not checked: BFL
/// allows a shorter or longer year when a business starts or changes year.
///
/// Closed periods live here, under `[fiscal_year]`, so "every closed month
/// lies inside the year" is one constructor's invariant. All fields are
/// private; [`FiscalYear::new`] is the only way in, and deserialisation goes
/// through it.
// VERIFY: BFL 3 kap. 1 § — that a räkenskapsår spans whole calendar months,
// and the bounds on a shortened or extended year. Check against the statute
// text at riksdagen.se.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FiscalYearWire")]
pub struct FiscalYear {
    start: NaiveDate,
    end: NaiveDate,
    closed: BTreeSet<Month>,
}

impl FiscalYear {
    /// A fiscal year from `start` to `end`, both inclusive, with the given
    /// months closed.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::FiscalYearReversed`] if `start` is after `end`.
    /// - [`ConfigError::FiscalYearNotMonthAligned`] if `start` is not the 1st
    ///   or `end` is not a month's last day.
    /// - [`ConfigError::ClosedPeriodOutsideFiscalYear`] for the first closed
    ///   month outside the year.
    /// - [`ConfigError::DuplicateClosedPeriod`] for the first month listed
    ///   twice.
    pub fn new(
        start: NaiveDate,
        end: NaiveDate,
        closed: &[Month],
    ) -> Result<FiscalYear, ConfigError> {
        if start > end {
            return Err(ConfigError::FiscalYearReversed { start, end });
        }
        // `succ_opt` is `None` only at `NaiveDate::MAX`, 31 December.
        let ends_a_month = end.succ_opt().is_none_or(|next| next.day() == 1);
        if start.day() != 1 || !ends_a_month {
            return Err(ConfigError::FiscalYearNotMonthAligned { start, end });
        }
        let (first, last) = (Month::containing(start), Month::containing(end));
        let mut set = BTreeSet::new();
        for &period in closed {
            if period < first || period > last {
                return Err(ConfigError::ClosedPeriodOutsideFiscalYear { period, start, end });
            }
            if !set.insert(period) {
                return Err(ConfigError::DuplicateClosedPeriod { period });
            }
        }
        Ok(FiscalYear {
            start,
            end,
            closed: set,
        })
    }

    /// The first day of the year.
    #[must_use]
    pub fn start(&self) -> NaiveDate {
        self.start
    }

    /// The last day of the year.
    #[must_use]
    pub fn end(&self) -> NaiveDate {
        self.end
    }

    /// The closed months, in chronological order.
    #[must_use]
    pub fn closed_periods(&self) -> &BTreeSet<Month> {
        &self.closed
    }
}

/// The TOML/JSON shape of `[fiscal_year]`, before it is proven.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FiscalYearWire {
    start: NaiveDate,
    end: NaiveDate,
    #[serde(default)]
    closed_periods: Vec<Month>,
}

impl TryFrom<FiscalYearWire> for FiscalYear {
    type Error = ConfigError;

    fn try_from(w: FiscalYearWire) -> Result<FiscalYear, ConfigError> {
        FiscalYear::new(w.start, w.end, &w.closed_periods)
    }
}
