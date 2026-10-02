//! Property and exhaustive tests for the event boundary parsers, over the
//! public API only.
//!
//! Each grammar is restated here independently of the parser — a predicate
//! on bytes, not a call into the crate — and the parser must agree with it
//! on every input. `CountryCode` is proven over every byte pair, as
//! `tests/account.rs` does over `u16` and `tests/verification.rs` over
//! `char`: the domain of a two-letter code is small enough to enumerate.
//!
//! Round-trips through serde for whole events arrive with the generators in
//! `kontera-testkit` (T6b); these cover the leaf types only.

use kontera_core::{CountryCode, EventId, ProviderId, VatNumber};
use proptest::prelude::*;

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
