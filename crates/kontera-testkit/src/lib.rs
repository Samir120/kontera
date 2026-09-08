//! Property-test generators shared accross Kontera's test crates.
//!
//! This crate exist because each file under `test/` compiles as its own
//! crate, and `kontera-sie`'s golden tests need generated ledgers across a
//! crate boundary. It is never published. See `docs/03-architecture.md` §3.1.
//!
//! `valid_event_log()` arrives in T6 and is itself tested: a generator that
//! emits a log `post()` rejects is a generator bug.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
