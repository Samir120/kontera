//! Tests for the config types, over the public API only.
//!
//! The leaf types come first. `Month` is proven over every `YYYY-MM` digit
//! string, a domain small enough to enumerate. `FiscalYear::new` is checked
//! against its rules restated as an independent predicate. The `Config` tree
//! tests join this file in T8b.

use chrono::{Datelike, Days, Months, NaiveDate};
use kontera_core::{ConfigError, FiscalYear, Month, OrgNumber};
use proptest::prelude::*;

#[test]
fn month_is_proven_over_every_digit_shape() {
    for year in 0..=9999 {
        for month in 0..=99 {
            let s = format!("{year:04}-{month:02}");
            let parsed = Month::parse(&s);
            assert_eq!(parsed.is_ok(), (1..=12).contains(&month), "{s}");
            if let Ok(m) = parsed {
                assert_eq!(m.to_string(), s);
            }
        }
    }
}

#[test]
fn month_rejects_other_shapes_and_orders_chronologically() {
    for s in [
        "2026-1",
        "202601",
        "2026/01",
        "2026-01-01",
        " 2026-01",
        "٢٠٢٦-٠١",
        "",
    ] {
        assert!(Month::parse(s).is_err(), "{s:?}");
    }
    assert!(Month::parse("2025-12").unwrap() < Month::parse("2026-01").unwrap());
    assert!(serde_json::from_str::<Month>("202601").is_err());
}

#[test]
fn org_number_is_any_non_empty_string() {
    assert_eq!(OrgNumber::parse(""), Err(ConfigError::EmptyOrgNumber));
    assert_eq!(
        OrgNumber::parse("XXXXXX-XXXX").unwrap().as_str(),
        "XXXXXX-XXXX"
    );
    assert!(serde_json::from_str::<OrgNumber>(r#""""#).is_err());
}

fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

fn month(s: &str) -> Month {
    Month::parse(s).unwrap()
}

#[test]
fn a_broken_fiscal_year_bounds_its_closed_periods() {
    let (start, end) = (date("2026-07-01"), date("2027-06-30"));
    let fy = FiscalYear::new(start, end, &[month("2027-06"), month("2026-07")]).unwrap();
    let closed: Vec<_> = fy
        .closed_periods()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(closed, ["2026-07", "2027-06"]);
    for outside in ["2026-06", "2027-07"] {
        let period = month(outside);
        assert_eq!(
            FiscalYear::new(start, end, &[period]),
            Err(ConfigError::ClosedPeriodOutsideFiscalYear { period, start, end })
        );
    }
}

#[test]
fn fiscal_year_rejections_name_their_cause() {
    let reversed = FiscalYear::new(date("2027-01-01"), date("2026-12-31"), &[]);
    assert!(matches!(
        reversed,
        Err(ConfigError::FiscalYearReversed { .. })
    ));
    for (s, e) in [
        ("2026-01-02", "2026-12-31"),
        ("2026-01-01", "2026-12-30"),
        ("2028-01-01", "2028-02-28"),
    ] {
        let r = FiscalYear::new(date(s), date(e), &[]);
        assert!(
            matches!(r, Err(ConfigError::FiscalYearNotMonthAligned { .. })),
            "{s}–{e}"
        );
    }
    let leap = FiscalYear::new(date("2028-01-01"), date("2028-02-29"), &[]);
    assert!(leap.is_ok());
    let jan = month("2026-01");
    let dup = FiscalYear::new(date("2026-01-01"), date("2026-12-31"), &[jan, jan]);
    assert_eq!(dup, Err(ConfigError::DuplicateClosedPeriod { period: jan }));
}

#[test]
fn fiscal_year_deserialises_through_its_constructor() {
    let fy: FiscalYear =
        serde_json::from_str(r#"{"start":"2026-01-01","end":"2026-12-31"}"#).unwrap();
    assert!(fy.closed_periods().is_empty());
    for bad in [
        r#"{"start":"2026-12-31","end":"2026-01-01"}"#,
        r#"{"start":"2026-01-01","end":"2026-12-31","closed_periods":["2025-12"]}"#,
        r#"{"start":"2026-01-01","end":"2026-12-31","closed":["2026-01"]}"#,
    ] {
        assert!(serde_json::from_str::<FiscalYear>(bad).is_err(), "{bad}");
    }
}

/// A first-of-month or any day, so aligned years are common, not rare.
fn arb_start() -> impl Strategy<Value = NaiveDate> {
    (2024i32..=2028, 1u32..=12, prop::option::of(2u32..=28))
        .prop_map(|(y, m, d)| NaiveDate::from_ymd_opt(y, m, d.unwrap_or(1)).unwrap())
}

/// A month's last day, or a day before it.
fn arb_end() -> impl Strategy<Value = NaiveDate> {
    (2024i32..=2029, 1u32..=12, 0u64..=2).prop_map(|(y, m, back)| {
        let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
        first + Months::new(1) - Days::new(1 + back)
    })
}

proptest! {
    #[test]
    fn fiscal_year_new_agrees_with_its_rules(
        start in arb_start(),
        end in arb_end(),
        months in prop::collection::vec((2023i32..=2030, 1u32..=12), 0..5),
    ) {
        let closed: Vec<Month> = months
            .iter()
            .map(|(y, m)| Month::parse(&format!("{y:04}-{m:02}")).unwrap())
            .collect();
        let key = |d: NaiveDate| (d.year(), d.month());
        let aligned = start.day() == 1 && (end + Days::new(1)).day() == 1;
        let inside = months.iter().all(|&m| key(start) <= m && m <= key(end));
        let mut sorted = months.clone();
        sorted.sort_unstable();
        sorted.dedup();
        let distinct = sorted.len() == months.len();
        let expected = start <= end && aligned && inside && distinct;
        prop_assert_eq!(FiscalYear::new(start, end, &closed).is_ok(), expected);
    }
}
