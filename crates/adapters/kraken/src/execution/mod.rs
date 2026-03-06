//! Live execution client implementations for the Kraken adapter.
//!
//! This module provides separate execution clients for Kraken Spot and Futures markets:
//!
//! - [`KrakenSpotExecutionClient`] - For Spot markets using WebSocket v2
//! - [`KrakenFuturesExecutionClient`] - For Futures markets
//!
//! # Supported Operations
//!
//! ## Common
//! - Order submission (market, limit, stop)
//! - Order modification
//! - Order cancellation (single, batch, cancel-all)
//! - Account state and balance queries
//!
//! ## Futures Only
//! - Position management

mod futures;
mod spot;

pub use futures::KrakenFuturesExecutionClient;
pub use spot::KrakenSpotExecutionClient;
