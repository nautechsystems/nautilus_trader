//! Kraken Futures WebSocket v1 API client implementation.
//!
//! Provides real-time futures market data and execution streams including:
//! - Order book snapshots and deltas
//! - Trade ticks and snapshots
//! - Quotes (best bid/ask)
//! - Mark price updates
//! - Index price updates
//! - Order status updates (open orders, cancellations)
//! - Fill reports

pub mod client;
pub mod handler;
pub mod messages;
