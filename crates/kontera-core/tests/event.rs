//! Property and exhaustive tests for the event boundary parsers, over the
//! public API only.
//!
//! Each grammar is restated here independently of the parser — a predicate
//! on bytes, not a call into the crate — and the parser must agree with it
//! on every input. `CountryCode` is proven over every byte pair, as
//! `tests/account.rs` does over `u16` and `tests/verification.rs` over
//! `char`: the domain of a two-letter code is small enough to enumerate.
//!
//! Below those, the wire rules of ADR-0006: `VatRate::Destination` is proven
//! over every percentage, and an unknown field is refused at every level of
//! every event.

use kontera_core::{
    BuyerTaxStatus, CountryCode, Event, EventId, ProviderId, SaleLine, VatNumber, VatPercent,
    VatRate,
};
use proptest::prelude::*;
use serde_json::Value;

/// The `ProviderId` grammar, restated: a lowercase TOML bare key.
fn is_bare_key(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
}

/// The `CountryCode` grammar, restated: exactly two ASCII capitals.
fn is_alpha2(s: &str) -> bool {
    s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase())
}

#[test]
fn country_code_is_proven_over_every_byte_pair() {
    let mut accepted = 0u32;
    for a in u8::MIN..=u8::MAX {
        for b in u8::MIN..=u8::MAX {
            let bytes = [a, b];
            // Pairs that are not UTF-8 cannot reach the parser at all.
            let Ok(s) = std::str::from_utf8(&bytes) else {
                continue;
            };
            let parsed = CountryCode::parse(s);
            assert_eq!(parsed.is_ok(), is_alpha2(s), "{s:?}");
            if let Ok(code) = parsed {
                assert_eq!(code.to_string(), s);
                accepted += 1;
            }
        }
    }
    assert_eq!(accepted, 26 * 26);
}

proptest! {
    #[test]
    fn every_parser_agrees_with_its_grammar(s in ".*") {
        prop_assert_eq!(EventId::parse(&s).is_ok(), !s.is_empty());
        prop_assert_eq!(VatNumber::parse(&s).is_ok(), !s.is_empty());
        prop_assert_eq!(ProviderId::parse(&s).is_ok(), is_bare_key(&s));
        prop_assert_eq!(CountryCode::parse(&s).is_ok(), is_alpha2(&s));
    }

    #[test]
    fn accepted_identifiers_round_trip_through_display_and_serde(s in ".*") {
        if let Ok(id) = EventId::parse(&s) {
            prop_assert_eq!(id.to_string(), s.clone());
            let json = serde_json::to_string(&id).unwrap();
            prop_assert_eq!(serde_json::from_str::<EventId>(&json).unwrap(), id);
        }
        if let Ok(id) = ProviderId::parse(&s) {
            prop_assert_eq!(id.to_string(), s.clone());
            let json = serde_json::to_string(&id).unwrap();
            prop_assert_eq!(serde_json::from_str::<ProviderId>(&json).unwrap(), id);
        }
    }
}

/// A sale line on the wire, with `vat_rate` as given.
fn line_with(vat_rate: &str) -> String {
    format!(
        r#"{{"description":"Widget","amount_excl_vat":"1000.00","vat_rate":{vat_rate},"goods_or_services":"goods"}}"#
    )
}

#[test]
fn every_destination_percentage_round_trips_byte_for_byte() {
    for hundredths in 1..=9999u32 {
        let percent = format!("{}.{:02}", hundredths / 100, hundredths % 100);
        let wire = line_with(&format!(r#"{{"destination":"{percent}"}}"#));
        let line: SaleLine = serde_json::from_str(&wire).unwrap();
        assert_eq!(
            line.vat_rate,
            VatRate::Destination(VatPercent::parse(&percent).unwrap())
        );
        assert_eq!(serde_json::to_string(&line).unwrap(), wire);
    }
}

#[test]
fn the_swedish_classes_keep_their_wire_form() {
    for (wire, rate) in [
        (r#""standard""#, VatRate::Standard),
        (r#""reduced12""#, VatRate::Reduced12),
        (r#""reduced6""#, VatRate::Reduced6),
        (r#""zero""#, VatRate::Zero),
    ] {
        let wire = line_with(wire);
        let line: SaleLine = serde_json::from_str(&wire).unwrap();
        assert_eq!(line.vat_rate, rate);
        assert_eq!(serde_json::to_string(&line).unwrap(), wire);
    }
}

#[test]
fn a_destination_rate_has_one_spelling_and_is_never_zero() {
    for vat_rate in [
        r#"{"destination":"0"}"#,
        r#"{"destination":"0.00"}"#,
        r#"{"destination":19}"#,
        r#"{"destination":19.0}"#,
        r#"{"destination":"100"}"#,
        r#"{"destination":null}"#,
        r#"{"destination":"19.00","standard":null}"#,
        r#"{"standard":"19.00"}"#,
        r#"{"percent":"19.00"}"#,
        r#"{}"#,
        r#""destination""#,
        r#""19.00""#,
        "19",
    ] {
        let line = serde_json::from_str::<SaleLine>(&line_with(vat_rate));
        assert!(line.is_err(), "{vat_rate}");
    }
}

/// One valid event per wire shape: both buyers, both line forms, all kinds.
const EVERY_SHAPE: [&str; 5] = [
    r#"{"type":"sale_captured","id":"a","at":"2026-01-05","provider":"stripe","buyer":{"kind":"consumer"},"ship_to":"SE","lines":[{"description":"x","amount_excl_vat":"1.00","vat_rate":"standard","goods_or_services":"goods"}]}"#,
    r#"{"type":"sale_captured","id":"b","at":"2026-01-05","provider":"stripe","buyer":{"kind":"business","vat_number":"X"},"ship_to":"XX","lines":[{"description":"x","amount_excl_vat":"1.00","vat_rate":{"destination":"20.00"},"goods_or_services":"goods"}]}"#,
    r#"{"type":"refund_issued","id":"c","at":"2026-01-06","original_sale":"b","lines":[{"description":"x","amount_excl_vat":"1.00","vat_rate":{"destination":"20.00"},"goods_or_services":"goods"}]}"#,
    r#"{"type":"fee_charged","id":"d","at":"2026-01-07","provider":"stripe","kind":"other","amount":"1.00"}"#,
    r#"{"type":"payout_settled","id":"e","at":"2026-01-08","provider":"stripe","amount":"1.00","covers":["a"]}"#,
];

/// The JSON pointer of every object in `value`, itself included.
fn objects(value: &Value, here: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            found.push(here.to_owned());
            for (key, child) in map {
                objects(child, &format!("{here}/{key}"), found);
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                objects(child, &format!("{here}/{i}"), found);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[test]
fn an_unknown_field_is_refused_at_every_level_of_every_event() {
    let mut levels = 0;
    for wire in EVERY_SHAPE {
        assert!(serde_json::from_str::<Event>(wire).is_ok(), "{wire}");
        let event: Value = serde_json::from_str(wire).unwrap();
        let mut found = Vec::new();
        objects(&event, "", &mut found);
        for pointer in found {
            let mut typo = event.clone();
            let object = typo.pointer_mut(&pointer).unwrap();
            object
                .as_object_mut()
                .unwrap()
                .insert("typo".to_owned(), Value::from(1));
            let parsed = serde_json::from_str::<Event>(&typo.to_string());
            assert!(parsed.is_err(), "typo at {pointer:?} in {wire}");
            levels += 1;
        }
    }
    // Consumer sale 3, business sale 4, refund 3, fee 1, payout 1.
    assert_eq!(levels, 12);
}

#[test]
fn a_consumer_buyer_refuses_an_unknown_field() {
    let consumer: BuyerTaxStatus = serde_json::from_str(r#"{"kind":"consumer"}"#).unwrap();
    assert_eq!(consumer, BuyerTaxStatus::Consumer);
    assert_eq!(
        serde_json::to_string(&consumer).unwrap(),
        r#"{"kind":"consumer"}"#
    );
    assert!(serde_json::from_str::<BuyerTaxStatus>(r#"{"kind":"consumer","typo":1}"#).is_err());
    // A consumer has no VAT number to carry, and a business must have one.
    assert!(
        serde_json::from_str::<BuyerTaxStatus>(r#"{"kind":"consumer","vat_number":"X"}"#).is_err()
    );
    assert!(serde_json::from_str::<BuyerTaxStatus>(r#"{"kind":"business"}"#).is_err());
}
