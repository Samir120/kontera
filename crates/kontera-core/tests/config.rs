//! Tests for the config types, over the public API only.
//!
//! The leaf types come first. `Month` is proven over every `YYYY-MM` digit
//! string, a domain small enough to enumerate. `FiscalYear::new` is checked
//! against its rules restated as an independent predicate. The `Config` tree
//! is parsed from TOML, as a host would, and its role-vs-class rules are
//! proven over every account number for every role.

use std::num::NonZeroU32;

use chrono::{Datelike, Days, Months, NaiveDate};
use kontera_core::{
    AccountKind, AccountNumber, Accounts, Company, Config, ConfigError, FiscalYear, Month,
    OrgNumber, ProviderId, Returns, SeRates, Series, SeriesConfig, VatConfig, VatPercent, VatRates,
};
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

/// Only the required keys.
const MINIMAL: &str = r#"
schema_version = 1

[company]
org_number = "XXXXXX-XXXX"
name = "Exempelbutiken AB"

[fiscal_year]
start = "2026-01-01"
end = "2026-12-31"

[series]
default = "A"
start_number = 1

[accounts]
bank = "1930"
provider_fee = "6570"

[accounts.receivable]
stripe = "1580"

[vat.rates.se]
standard = "25"
reduced12 = "12"
reduced6 = "6"
"#;

/// Every table and key. The OSS accounts are arbitrary numbers of the right
/// class, not a mapping (Q8).
const FULL: &str = r#"
schema_version = 1

[company]
org_number = "XXXXXX-XXXX"
name = "Exempelbutiken AB"

[fiscal_year]
start = "2026-01-01"
end = "2026-12-31"
closed_periods = ["2026-01"]

[series]
default = "A"
start_number = 1

[accounts]
bank = "1930"
provider_fee = "6570"

[accounts.receivable]
stripe = "1580"
klarna = "1581"

[accounts.sales]
eu_b2b_reverse = "3108"
export_outside_eu = "3105"
eu_b2c_oss = "3999"

[accounts.sales.se]
standard = "3001"
reduced12 = "3002"
reduced6 = "3003"

[accounts.vat_payable]
eu_b2c_oss = "2999"

[accounts.vat_payable.se]
standard = "2611"
reduced12 = "2621"
reduced6 = "2631"

[returns]
strategy = "reverse_original"

[vat.rates.se]
standard = "25.00"
reduced12 = "12.00"
reduced6 = "6.00"
"#;

fn parse(toml: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(toml)
}

fn konto(s: &str) -> AccountNumber {
    AccountNumber::parse(s).unwrap()
}

#[test]
fn a_minimal_config_parses_with_every_optional_mapping_none() {
    let c = parse(MINIMAL).unwrap();
    let a = c.accounts();
    assert_eq!((a.bank, a.provider_fee), (konto("1930"), konto("6570")));
    assert_eq!(a.receivable.len(), 1);
    assert_eq!(a.sales.se, SeRates::default());
    assert_eq!(
        (
            a.sales.eu_b2b_reverse,
            a.sales.export_outside_eu,
            a.sales.eu_b2c_oss
        ),
        (None, None, None)
    );
    assert_eq!(a.vat_payable.se, SeRates::default());
    assert_eq!(a.vat_payable.eu_b2c_oss, None);
    assert_eq!(c.returns(), Returns::ReverseOriginal);
    assert_eq!(c.series().start_number.get(), 1);
    assert_eq!(
        c.vat().rates.se.reduced6,
        VatPercent::parse("6.00").unwrap()
    );
}

#[test]
fn a_full_config_reads_every_key() {
    let c = parse(FULL).unwrap();
    let a = c.accounts();
    let receivable: Vec<_> = a
        .receivable
        .iter()
        .map(|(p, k)| (p.as_str(), k.get()))
        .collect();
    assert_eq!(receivable, [("klarna", 1581), ("stripe", 1580)]);
    assert_eq!(a.sales.se.reduced12, Some(konto("3002")));
    assert_eq!(a.sales.eu_b2b_reverse, Some(konto("3108")));
    assert_eq!(a.sales.export_outside_eu, Some(konto("3105")));
    assert_eq!(a.sales.eu_b2c_oss, Some(konto("3999")));
    assert_eq!(a.vat_payable.se.reduced6, Some(konto("2631")));
    assert_eq!(a.vat_payable.eu_b2c_oss, Some(konto("2999")));
    assert_eq!(c.fiscal_year().closed_periods().len(), 1);
    assert_eq!(c.company().name, "Exempelbutiken AB");
}

#[test]
fn schema_version_must_be_present_and_equal_to_one() {
    let missing = MINIMAL.replace("schema_version = 1\n", "");
    assert!(parse(&missing).is_err());
    let two = parse(&MINIMAL.replace("schema_version = 1", "schema_version = 2")).unwrap_err();
    assert!(
        two.to_string()
            .contains("config `schema_version` is 2; this version of Kontera reads 1"),
        "{two}"
    );
}

#[test]
fn an_unknown_key_is_rejected_at_every_level() {
    let headers: Vec<&str> = FULL.lines().filter(|l| l.starts_with('[')).collect();
    assert_eq!(headers.len(), 11, "every explicit table in FULL is covered");
    let top = format!("typo = \"1\"\n{FULL}");
    assert!(parse(&top).is_err(), "top level");
    // `[vat]` and `[vat.rates]` exist only implicitly in FULL; TOML lets a
    // parent table be written after its child.
    for parent in ["[vat]", "[vat.rates]"] {
        let typo = format!("{FULL}\n{parent}\ntypo = \"1\"\n");
        assert!(parse(&typo).is_err(), "{parent}");
        let empty = format!("{FULL}\n{parent}\n");
        assert!(
            parse(&empty).is_ok(),
            "{parent} without a typo still parses"
        );
    }
    for header in headers {
        let typo = FULL.replace(&format!("{header}\n"), &format!("{header}\ntypo = \"1\"\n"));
        assert!(parse(&typo).is_err(), "{header}");
    }
}

#[test]
fn boundary_values_are_rejected_by_their_parsers() {
    for (from, to) in [
        ("stripe = \"1580\"", "Stripe = \"1580\""),
        ("start_number = 1", "start_number = 0"),
        ("bank = \"1930\"", "bank = 1930"),
        ("standard = \"25\"", "standard = 25"),
        ("reduced6 = \"6\"\n", ""),
    ] {
        assert!(parse(&MINIMAL.replace(from, to)).is_err(), "{to:?}");
    }
}

#[test]
fn returns_has_one_strategy_and_defaults_to_it() {
    let named = format!("{MINIMAL}\n[returns]\nstrategy = \"reverse_original\"\n");
    assert_eq!(parse(&named).unwrap().returns(), Returns::ReverseOriginal);
    let other = format!("{MINIMAL}\n[returns]\nstrategy = \"dedicated_account\"\n");
    assert!(parse(&other).is_err());
}

/// The class a role needs, restated from docs/04 §3 by the first digit,
/// independent of `AccountNumber::class`.
fn digit_allows(role: AccountKind, n: u16) -> bool {
    let digit = n / 1000;
    match role {
        AccountKind::Asset => digit == 1,
        AccountKind::EquityAndLiability => digit == 2,
        AccountKind::Income => digit == 3,
        AccountKind::Expense => (4..=7).contains(&digit),
        AccountKind::FinancialItem => digit == 8,
    }
}

/// Every account role in `Accounts`: its config key and the class it needs.
const ROLES: [(&str, AccountKind); 13] = {
    use AccountKind::{Asset, EquityAndLiability, Expense, Income};
    [
        ("accounts.bank", Asset),
        ("accounts.provider_fee", Expense),
        ("accounts.receivable.stripe", Asset),
        ("accounts.sales.se.standard", Income),
        ("accounts.sales.se.reduced12", Income),
        ("accounts.sales.se.reduced6", Income),
        ("accounts.sales.eu_b2b_reverse", Income),
        ("accounts.sales.export_outside_eu", Income),
        ("accounts.sales.eu_b2c_oss", Income),
        ("accounts.vat_payable.se.standard", EquityAndLiability),
        ("accounts.vat_payable.se.reduced12", EquityAndLiability),
        ("accounts.vat_payable.se.reduced6", EquityAndLiability),
        ("accounts.vat_payable.eu_b2c_oss", EquityAndLiability),
    ]
};

fn stripe() -> ProviderId {
    ProviderId::parse("stripe").unwrap()
}

/// Put `k` in the slot `key` names.
fn assign(a: &mut Accounts, key: &str, k: AccountNumber) {
    let some = Some(k);
    match key {
        "accounts.bank" => a.bank = k,
        "accounts.provider_fee" => a.provider_fee = k,
        "accounts.receivable.stripe" => {
            a.receivable.insert(stripe(), k);
        }
        "accounts.sales.se.standard" => a.sales.se.standard = some,
        "accounts.sales.se.reduced12" => a.sales.se.reduced12 = some,
        "accounts.sales.se.reduced6" => a.sales.se.reduced6 = some,
        "accounts.sales.eu_b2b_reverse" => a.sales.eu_b2b_reverse = some,
        "accounts.sales.export_outside_eu" => a.sales.export_outside_eu = some,
        "accounts.sales.eu_b2c_oss" => a.sales.eu_b2c_oss = some,
        "accounts.vat_payable.se.standard" => a.vat_payable.se.standard = some,
        "accounts.vat_payable.se.reduced12" => a.vat_payable.se.reduced12 = some,
        "accounts.vat_payable.se.reduced6" => a.vat_payable.se.reduced6 = some,
        "accounts.vat_payable.eu_b2c_oss" => a.vat_payable.eu_b2c_oss = some,
        other => panic!("no account slot `{other}`"),
    }
}

fn build(accounts: Accounts) -> Result<Config, ConfigError> {
    let pct = |s| VatPercent::parse(s).unwrap();
    Config::new(
        Company {
            org_number: OrgNumber::parse("XXXXXX-XXXX").unwrap(),
            name: "Exempelbutiken AB".to_owned(),
        },
        FiscalYear::new(date("2026-01-01"), date("2026-12-31"), &[]).unwrap(),
        SeriesConfig {
            default: Series::parse("A").unwrap(),
            start_number: NonZeroU32::MIN,
        },
        accounts,
        Returns::ReverseOriginal,
        VatConfig {
            rates: VatRates {
                se: SeRates {
                    standard: pct("25"),
                    reduced12: pct("12"),
                    reduced6: pct("6"),
                },
            },
        },
    )
}

#[test]
fn every_role_admits_exactly_its_class_over_every_account() {
    let base = parse(MINIMAL).unwrap().accounts().clone();
    for (key, expected) in ROLES {
        for n in 1000..=8999 {
            let account = AccountNumber::try_from(n).unwrap();
            let mut accounts = base.clone();
            assign(&mut accounts, key, account);
            let shared = accounts.receivable[&stripe()] == accounts.bank;
            let result = build(accounts);
            let want = if !digit_allows(expected, n) {
                Err(ConfigError::WrongAccountClass {
                    key: key.to_owned(),
                    account,
                    expected,
                })
            } else if shared {
                Err(ConfigError::BankIsReceivable {
                    provider: stripe(),
                    account,
                })
            } else {
                Ok(())
            };
            assert_eq!(result.map(|_| ()), want, "{key} = {n}");
        }
    }
}

#[test]
fn role_errors_name_the_key_and_both_classes() {
    let bad = MINIMAL.replace("provider_fee = \"6570\"", "provider_fee = \"2611\"");
    let e = parse(&bad).unwrap_err().to_string();
    assert!(
        e.contains("`accounts.provider_fee` is 2611, which is in class 2 (equity and liabilities); this role needs classes 4–7 (expenses)"),
        "{e}"
    );
    let shared = MINIMAL.replace("stripe = \"1580\"", "stripe = \"1930\"");
    let e = parse(&shared).unwrap_err().to_string();
    assert!(
        e.contains("`accounts.bank` and `accounts.receivable.stripe` are both 1930"),
        "{e}"
    );
}
