//! Binance Spot WebSocket clients with SBE (Simple Binary Encoding) support.
//!
//! This module provides two WebSocket clients for Binance Spot:
//!
//! ## Market Data Streams (`streams`)
//!
//! Pub/sub pattern for real-time market data via `stream-sbe.binance.com`:
//! - Trade streams
//! - Best bid/offer updates
//! - Order book depth updates
//!
//! ## Trading API (`trading`)
//!
//! Request/response pattern for order management via `ws-api.binance.com`:
//! - Order placement
//! - Order cancellation
//! - Cancel-replace operations

pub mod error;
pub mod streams;
pub mod trading;

pub use streams::BinanceSpotWebSocketClient;
pub use trading::BinanceSpotWsTradingClient;
