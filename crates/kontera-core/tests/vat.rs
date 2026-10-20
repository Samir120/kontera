//! Exhaustive tests for `VatPercent`, over the public API only.
//!
//! The grammar is restated here as an independent predicate, and the parser
//! must agree with it on every string of up to five characters drawn from
//! `0-9` and `.` — 177 156 strings, which covers every accepted value and
//! every way to misplace a digit or a point. Characters outside that alphabet
//! are spot-checked.

use kontera_core::VatPercent;

/// The `VatPercent` grammar, restated: the value in hundredths if accepted.
fn reference_percent(s: &str) -> Option<u32> {
    let mut parts = s.split('.');
    let int = parts.next()?;
    let frac = parts.next();
    if parts.next().is_some() || int.is_empty() || int.len() > 2 {
        return None;
    }
    if int.len() == 2 && int.starts_with('0') {
        return None;
    }
    let digits = |p: &str| p.chars().all(|c| c.is_ascii_digit());
    let hundredths = match frac {
        None => 0,
        Some(f) if (1..=2).contains(&f.len()) && digits(f) => {
            format!("{f:0<2}").parse::<u32>().ok()?
        }
        Some(_) => return None,
    };
    let value = int.parse::<u32>().ok()? * 100 + hundredths;
    (digits(int) && value > 0).then_some(value)
}

#[test]
fn vat_percent_is_proven_over_every_short_string() {
    const ALPHABET: &[u8] = b"0123456789.";
    let mut strings = vec![String::new()];
    let mut frontier = strings.clone();
    for _ in 0..5 {
        frontier = frontier
            .iter()
            .flat_map(|s| ALPHABET.iter().map(move |&c| format!("{s}{}", c as char)))
            .collect();
        strings.extend(frontier.iter().cloned());
    }
    let mut accepted = 0;
    for s in &strings {
        let parsed = VatPercent::parse(s);
        match reference_percent(s) {
            Some(v) => {
                let shown = format!("{}.{:02}", v / 100, v % 100);
                assert_eq!(parsed.unwrap().to_string(), shown, "{s:?}");
                accepted += 1;
            }
            None => assert!(parsed.is_err(), "{s:?}"),
        }
    }
    // 100 integer parts × 111 fraction forms, less "0", "0.0" and "0.00".
    assert_eq!(accepted, 11_097);
}

#[test]
fn vat_percent_rejects_what_the_alphabet_cannot_spell() {
    for s in ["+25", "-1", " 25", "25 ", "1e1", "25,00", "٢٥"] {
        assert!(VatPercent::parse(s).is_err(), "{s:?}");
    }
}

#[test]
fn vat_percent_deserialises_from_a_string_only() {
    let p: VatPercent = serde_json::from_str(r#""25""#).unwrap();
    assert_eq!(p, VatPercent::parse("25.00").unwrap());
    assert!(serde_json::from_str::<VatPercent>("25").is_err());
    assert!(serde_json::from_str::<VatPercent>("25.0").is_err());
}
