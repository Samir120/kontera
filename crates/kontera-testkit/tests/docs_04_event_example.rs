//! The event example in docs/04 §2.2 parses, round-trips, and means what it
//! says.
//!
//! Field order in the structs matches the document, so serialising back must
//! give the same bytes. It lives here for the same reason as the config
//! example test: `include_str!` reaching out to `docs/` would break the tests
//! of a packaged `kontera-core`.

use chrono::NaiveDate;
use kontera_core::{
    BuyerTaxStatus, CountryCode, Event, EventId, FeeKind, Money, VatPercent, VatRate,
};

const DOC: &str = include_str!("../../../docs/04-public-api.md");

/// The lines of the one ```` ```jsonl ```` block in the doc.
fn example() -> Vec<&'static str> {
    let mut blocks = DOC.split("```jsonl\n").skip(1);
    let block = blocks.next().expect("docs/04 has a ```jsonl block");
    assert!(
        blocks.next().is_none(),
        "docs/04 has exactly one ```jsonl block"
    );
    let (jsonl, _) = block.split_once("\n```").expect("the block is closed");
    jsonl.lines().collect()
}

fn kr(s: &str) -> Money {
    Money::parse(s).unwrap()
}

fn parse(line: &str) -> Event {
    serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}"))
}

#[test]
fn every_docs_04_example_line_round_trips_byte_for_byte() {
    for line in example() {
        assert_eq!(serde_json::to_string(&parse(line)).unwrap(), line);
    }
}

#[test]
fn the_docs_04_example_lines_mean_what_they_say() {
    let lines = example();
    let [sale, oss, fee, payout] = lines.as_slice() else {
        panic!("docs/04 §2.2 shows four events, found {}", lines.len());
    };

    let Event::SaleCaptured(s) = parse(sale) else {
        panic!("tag `sale_captured` must produce SaleCaptured");
    };
    assert_eq!(s.id, EventId::parse("ord_1001").unwrap());
    assert_eq!(s.at, NaiveDate::from_ymd_opt(2026, 1, 5).unwrap());
    assert_eq!(s.buyer, BuyerTaxStatus::Consumer);
    assert_eq!(s.ship_to, CountryCode::SE);
    assert_eq!(s.lines.len(), 1);
    assert_eq!(s.lines[0].amount_excl_vat(), kr("1000.00"));
    assert_eq!(s.lines[0].vat_rate, VatRate::Standard);

    let Event::SaleCaptured(o) = parse(oss) else {
        panic!("tag `sale_captured` must produce SaleCaptured");
    };
    assert_eq!(o.ship_to, CountryCode::parse("XX").unwrap());
    assert_eq!(
        o.lines[0].vat_rate,
        VatRate::Destination(VatPercent::parse("20.00").unwrap())
    );

    let Event::FeeCharged(f) = parse(fee) else {
        panic!("tag `fee_charged` must produce FeeCharged");
    };
    assert_eq!(f.kind, FeeKind::Transaction);
    assert_eq!(f.amount(), kr("1168.00"));

    let Event::PayoutSettled(p) = parse(payout) else {
        panic!("tag `payout_settled` must produce PayoutSettled");
    };
    assert_eq!(p.amount, kr("47832.00"));
    assert_eq!(p.covers.len(), 2);
    assert_eq!(p.covers[1], EventId::parse("fee_2026_01_08").unwrap());
}
