//! DeFi (Decentralized Finance) integration for the data crate.
//!
//! This module provides centralized access to DeFi functionality throughout the data crate.
//! DeFi support includes client subscriptions and engine processing.
//!
//! # Feature Flag
//!
//! All DeFi functionality requires the `defi` feature flag to be enabled:
//! ```toml
//! nautilus-data = { version = "0.x", features = ["defi"] }
//! ```

pub mod client;
pub mod engine;
