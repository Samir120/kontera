// crates/kontera-core/src/money.rs
//! `Money` — decimal amounts in öre. See ADR-0002 and docs/02 §2.1.
//!
//! Rounding is half-away-from-zero, applied per line and then summed. Never
//! sum then round: the results differ, and only the per-line result matches
//! what the host's checkout charged.
