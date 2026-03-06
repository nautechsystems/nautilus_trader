//! Live market data client implementations for the Kraken adapter.
//!
//! This module provides separate data clients for Kraken Spot and Futures markets:
//!
//! - [`KrakenSpotDataClient`] - For Spot markets using WebSocket v2
//! - [`KrakenFuturesDataClient`] - For Futures markets
//!
//! # Supported Data Types
//!
//! ## Spot
//! - Order book deltas and snapshots
//! - Trade ticks
//! - Quote ticks (best bid/ask)
//! - OHLC bars
//!
//! ## Futures
//! - Order book deltas and snapshots
//! - Trade ticks
//! - Quote ticks (best bid/ask)
//! - Mark prices
//! - Index prices

mod futures;
mod spot;

pub use futures::KrakenFuturesDataClient;
pub use spot::KrakenSpotDataClient;
