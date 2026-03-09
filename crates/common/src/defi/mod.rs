//! DeFi (Decentralized Finance) integration for NautilusTrader.
//!
//! This module provides centralized access to DeFi functionality throughout the common crate.
//! DeFi support includes:
//!
//! # Feature Flag
//!
//! All DeFi functionality requires the `defi` feature flag to be enabled:
//! ```toml
//! nautilus-common = { version = "0.x", features = ["defi"] }
//! ```

pub mod cache;
pub mod data_actor;
pub mod switchboard;

// Re-exports
// Re-exports
pub use switchboard::{
    get_defi_blocks_topic, get_defi_collect_topic, get_defi_flash_topic, get_defi_liquidity_topic,
    get_defi_pool_swaps_topic, get_defi_pool_topic,
};

pub use crate::messages::defi::*;
