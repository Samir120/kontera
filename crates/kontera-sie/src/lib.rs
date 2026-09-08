//! SIE 4I output for kontera ledgers.
//!
//! Emits type 4I - verification only, as produces by a *försystem* - encoded
//! as CP437 bytes rather than a `String`, because CP437 output is not valid
//! UTF-8. See `docs/adr/0003-sie-4i-output.md`.
//!
//! This crate is subject to the same purity rules as `kontera-core`: the
//! `#GEN`date is a parameter, never a clock read.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod cp437;
mod writer;
