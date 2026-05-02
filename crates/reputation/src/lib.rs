//! ERC-8004 reputation attestation layer for KuaaMU Quant Engine.
//!
//! Each trade decision and outcome is attested on-chain.
//! Reputation score determines agent autonomy level (Slider).

pub mod attestation;
pub mod autonomy;
pub mod client;

pub use attestation::{Attestation, TradeOutcome};
pub use autonomy::{AutonomyLevel, AutonomySlider};
pub use client::ReputationClient;
