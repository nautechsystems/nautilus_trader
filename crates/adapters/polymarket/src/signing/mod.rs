//! Polymarket EIP-712 (L1) signing.
//!
//! L2 HMAC-SHA256 signing lives on [`Credential`](crate::common::credential::Credential).

pub mod eip712;
pub mod hmac;
