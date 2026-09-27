//! Property tests for `BalancedTransaction` and `Series`, over the public API
//! only.
//!
//!This is the M1 exit criterion (docs/06): an unbalanced verification cannot
//!be constructed, proven over generated line sets. As in `tests/money.rs`,
//!every property has an independent reference next to it — here the sum and
//!the totals are recomputed in `i128` öre, never through `Money`.
//!
//!`Series` is checked exhaustively over every `char`, as `tests/account.rs`
//! does over `u16`: a loop over the whole domain is a proof, not a sample.

use kontera_core::{AccountNumber, BalancedTransaction, Line, Money, Series, VerificationError};
use proptest::prelude::*;

/// At most this many generated lines per set.
const LINES: usize = 32;

/// Per-line bound: `LINES` lines, one closing line and one perturbation all
/// stay inside `i64` öre, so every derived amount can be built with
/// `Money::from_ore` and no reference sum overflows.
const BOUND: i64 = i64::MAX / 64;

fn arb_account() -> impl Strategy<Value = AccountNumber> {
    (1000u16..=8999).prop_map(|n| AccountNumber::try_from(n).unwrap())
}

fn arb_line() -> impl Strategy<Value = Line> {
    (arb_account(), -BOUND..=BOUND).prop_map(|(account, ore)| Line {
        account,
        amount: Money::from_ore(ore),
    })
}

/// A non-empty line set. Almost never balanced — that is what the closing
/// line in [`arb_balanced`] is for.
fn arb_lines() -> impl Strategy<Value = Vec<Line>> {
    prop::collection::vec(arb_line(), 1..=LINES)
}

/// Any line set plus one closing line that absorb its sum. Always balanced.
fn arb_balanced() -> impl Strategy<Value = Vec<Line>> {
    (arb_lines(), arb_account()).prop_map(|(mut lines, account)| {
        lines.push(Line {
            account,
            amount: -ore(reference_sum(&lines)),
        });
        lines
    })
}

/// `Money` from an `i128` the bounds above keep inside `i64`
fn ore(n: i128) -> Money {
    Money::from_ore(i64::try_from(n).expect("bound by LINES * BOUND"))
}

/// Reference_sum, signed, in `i128` öre.
fn reference_sum(lines: &[Line]) -> i128 {
    lines.iter().map(|line| line.amount.ore()).sum()
}

/// Reference debit and credit magnitudes, in `i128` öre.
fn reference_totals(lines: &[Line]) -> (i128, i128) {
    lines
        .iter()
        .map(|line| line.amount.ore())
        .fold((0, 0), |(debit, credit), o| {
            if o < 0 {
                (debit, credit - o)
            } else {
                (debit + o, credit)
            }
        })
}

/// Reference for `Series::parse`: docs/02 §1, "a letter", read the slow way.
fn reference_series(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let first = chars.next()?;
    (chars.next().is_none() && first.is_ascii_uppercase()).then_some(first)
}

fn expected_series(s: &str) -> Result<Series, VerificationError> {
    reference_series(s)
        .map(|c| Series::parse(&c.to_string()).unwrap())
        .ok_or_else(|| VerificationError::InvalidSeries {
            input: s.to_owned(),
        })
}

/// Every single-character string, exhaustively: exactly the 26 ASCII capitals
/// parse, each displays as itself, and each round-trips through serde.
#[test]
fn every_single_char_string_parses_per_the_reference() {
    let mut accepted = 0;
    for c in char::MIN..=char::MAX {
        let s = c.to_string();
        let parsed = Series::parse(&s);
        assert_eq!(parsed, expected_series(&s), "{s:?}");
        if let Ok(series) = parsed {
            accepted += 1;
            assert_eq!(series.get(), c);
            assert_eq!(series.to_string(), s);
            let json = serde_json::to_string(&series).unwrap();
            assert_eq!(json, format!("\"{c}\""));
            assert_eq!(serde_json::from_str::<Series>(&json).unwrap(), series);
        }
    }
    assert_eq!(accepted, 26);
}

proptest! {
    /// The constructor accepts a non-empty line set exactly when its `i128`
    /// öre sum is zero, keeps the lines as given, and otherwise reports the
    /// independently computed debit and credit totals. `Empty` never appears
    /// for non-empty input.
    #[test]
    fn accepts_exactly_the_zero_sum_line_sets(lines in arb_lines()) {
        let sum = reference_sum(&lines);
        let (debit, credit) = reference_totals(&lines);
        prop_assert_eq!(debit - credit, sum);
        let expected = if sum == 0 {
            Ok(lines.clone())
        } else {
            Err(VerificationError::Unbalanced {
                debit: ore(debit),
                credit: ore(credit),
            })
        };
        let actual = BalancedTransaction::new(lines).map(|bt| bt.lines().to_vec());
        prop_assert_eq!(actual, expected);
    }

    /// Every line set can be closed by one line, and the result is accepted
    /// with the lines in the given order — the constructor is not simply
    /// rejecting everything. The accepted lines sum to `Money::ZERO`: I1.
    #[test]
    fn a_closing_line_balances_any_line_set(lines in arb_balanced()) {
        prop_assert_eq!(reference_sum(&lines), 0);
        let bt = BalancedTransaction::new(lines.clone());
        prop_assert_eq!(bt.as_ref().map(|bt| bt.lines()), Ok(lines.as_slice()));
        let posted: Money = bt.unwrap().lines().iter().map(|line| line.amount).sum();
        prop_assert_eq!(posted, Money::ZERO);
    }

    /// I1 is tight: moving any one line of a balanced set by any non-zero
    /// amount — down to a single öre — is rejected, and the reported totals
    /// differ by exactly that amount.
    #[test]
    fn perturbing_one_line_of_a_balanced_set_is_always_rejected(
        lines in arb_balanced(),
        index in any::<prop::sample::Index>(),
        delta in (-BOUND..=BOUND).prop_filter("non-zero", |d| *d!= 0),
    ) {
        let mut tampered = lines;
        let i = index.index(tampered.len());
        tampered[i].amount = tampered[i].amount + Money::from_ore(delta);
        let (debit, credit) = reference_totals(&tampered);
        prop_assert_eq!(debit - credit, i128::from(delta));
        prop_assert_eq!(
        BalancedTransaction::new(tampered),
            Err(VerificationError::Unbalanced {
            debit: ore(debit),
            credit: ore(credit),
        })
    );
    }

    /// Arbitrary strings — any length, any Unicode — parse per the reference.
    #[test]
    fn arbitrary_strings_parse_as_series_per_the_reference(s in ".*") {
        prop_assert_eq!(Series::parse(&s), expected_series(&s));
    }
}
