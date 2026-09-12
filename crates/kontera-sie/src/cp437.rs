// crates/kontera-sie/src/cp437.rs
//! CP437 encoding. See ADR-0003.
//!
//! Unrepresentable characters return `Err`, never a substituted `?` — silent
//! substitution in räkenskapsinformation is worse than a failed export.
