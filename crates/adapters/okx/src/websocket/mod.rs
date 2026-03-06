//! WebSocket client implementation for the OKX v5 API.
//!
//! This module provides real-time streaming connectivity to OKX WebSocket endpoints,
//! supporting:
//!
//! - Market data streaming (order books, trades, tickers, bars).
//! - Private data streaming (account updates, positions, orders).
//! - Order management (place, cancel, amend) via WebSocket.
//! - Authentication and automatic reconnection.
//! - Channel subscription management.

pub mod client;
pub mod enums;
pub mod error;
pub mod handler;
pub mod messages;
pub mod parse;
pub mod subscription;

// Re-exports
pub use crate::websocket::client::OKXWebSocketClient;
