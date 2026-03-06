//! Kraken Spot WebSocket v2 API client implementation.
//!
//! Provides real-time market data streams including:
//! - Ticker (quotes)
//! - Trades
//! - Order book
//! - OHLC bars

pub mod client;
pub mod enums;
pub mod handler;
pub mod messages;
pub mod parse;
