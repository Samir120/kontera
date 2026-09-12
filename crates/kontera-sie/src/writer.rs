// crates/kontera-sie/src/writer.rs
//! SIE 4I record emission: header records, `#VER`, `#TRANS`.
//! See docs/04 §6.
//!
//! The `#GEN` date is a parameter. A clock read here breaks byte-comparison
//! in the golden tests.
