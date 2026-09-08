//! Swedish bookkeeping as a pure function
//!
//! `kontera-core` folds a commerce event log into balanced verification on BAS
//! accounts. It performs no I/O, holds no state, reads no clock, and spawn no
//! tasks - see `docs/adr/0001-pure-core.md`. Deteminism is a correctness
//! requirement here, not a preference: golden test compare bytes.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Exhaustive matching on domain enums is a compliance mechanism, not a style
// rule: a wildcard arm turns "a new VAT scenario wad added" from a build
// failure into a silently wrong momsdeklaration. See docs/08 §6.
#![deny(clippy::wildcard_enum_match_arm)]

mod account;
mod config;
mod error;
mod event;
mod ledger;
mod money;
mod rules;
mod settle;
mod vat;
mod verification;

// Public re-exports land here as each is filled in. `post()`is written
// in M2 once rules and settle exist; declaring it now would be a signature
// invented ahead of the types it depends on.
