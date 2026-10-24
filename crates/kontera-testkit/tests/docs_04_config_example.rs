//! The config example in docs/04 §3 parses, and means what it says.
//!
//! Hosts copy that block, so it is pinned to the types: rename a key or
//! tighten a parser and this fails until the doc is fixed too. It lives here,
//! not in `kontera-core`, because `include_str!` reaching out of the crate to
//! `docs/` breaks the tests of a packaged crate. Testkit is never published.

use kontera_core::{AccountNumber, Config, Month, Returns, SeRates, VatPercent};

const DOC: &str = include_str!("../../../docs/04-public-api.md");

/// The one ```` ```toml ```` block in the doc.
fn example() -> &'static str {
    let mut blocks = DOC.split("```toml\n").skip(1);
    let block = blocks.next().expect("docs/04 has a ```toml block");
    assert!(
        blocks.next().is_none(),
        "docs/04 has exactly one ```toml block"
    );
    let (toml, _) = block.split_once("\n```").expect("the block is closed");
    toml
}

fn konto(s: &str) -> AccountNumber {
    AccountNumber::parse(s).unwrap()
}

fn some(s: &str) -> Option<AccountNumber> {
    Some(konto(s))
}

fn pct(s: &str) -> VatPercent {
    VatPercent::parse(s).unwrap()
}

#[test]
fn the_docs_04_config_example_parses_to_exactly_its_values() {
    let c: Config = toml::from_str(example()).unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(c.company().org_number.as_str(), "XXXXXX-XXXX");
    assert_eq!(c.company().name, "Exempelbutiken AB");

    let fy = c.fiscal_year();
    assert_eq!(fy.start().to_string(), "2026-01-01");
    assert_eq!(fy.end().to_string(), "2026-12-31");
    let closed: Vec<Month> = fy.closed_periods().iter().copied().collect();
    assert_eq!(closed, [Month::parse("2026-01").unwrap()]);

    assert_eq!(c.series().default.get(), 'A');
    assert_eq!(c.series().start_number.get(), 1);

    let a = c.accounts();
    assert_eq!(a.bank, konto("1930"));
    assert_eq!(a.provider_fee, konto("6570"));
    let receivable: Vec<_> = a
        .receivable
        .iter()
        .map(|(p, k)| (p.as_str(), k.get()))
        .collect();
    assert_eq!(receivable, [("klarna", 1581), ("stripe", 1580)]);

    let s = &a.sales;
    let se = SeRates {
        standard: some("3001"),
        reduced12: some("3002"),
        reduced6: some("3003"),
    };
    assert_eq!(s.se, se);
    assert_eq!(s.eu_b2b_reverse, some("3108"));
    assert_eq!(s.export_outside_eu, some("3105"));

    let v = &a.vat_payable;
    let se = SeRates {
        standard: some("2611"),
        reduced12: some("2621"),
        reduced6: some("2631"),
    };
    assert_eq!(v.se, se);

    assert_eq!(c.returns(), Returns::ReverseOriginal);

    let rates = SeRates {
        standard: pct("25.00"),
        reduced12: pct("12.00"),
        reduced6: pct("6.00"),
    };
    assert_eq!(c.vat().rates.se, rates);
}

/// No real BAS number may stand in for an OSS account (02 §4, Q8). The
/// example shows where the keys go, commented out, with no value.
#[test]
fn the_docs_04_example_maps_no_oss_account() {
    let c: Config = toml::from_str(example()).unwrap();
    assert_eq!(c.accounts().sales.eu_b2c_oss, None);
    assert_eq!(c.accounts().vat_payable.eu_b2c_oss, None);
    for line in example().lines().filter(|l| l.contains("eu_b2c_oss")) {
        let (key, rest) = line.split_once('=').expect("shown as `key =`");
        assert!(key.trim_start().starts_with('#'), "commented out: {line}");
        assert!(!rest.contains('"'), "no value: {line}");
    }
}
