// crates/kontera-core/src/settle.rs
//! Settlement reconciliation — the I2 invariant. See docs/02 §6.
//!
//! `payout == Σ sales − Σ refunds − Σ fees` for the covered set. This is the
//! invariant the library exists to enforce.
