//! WebSocket client implementation for BitMEX real-time data feeds.
//!
//! This module provides a WebSocket client for subscribing to BitMEX's real-time data streams.
//! It supports:
//! - Public market data subscriptions (trades, quotes, order book updates).
//! - Private account data subscriptions (orders, positions, executions).
//! - Authentication for private channels.
//! - Automatic reconnection and subscription management.
//! - Message parsing into Nautilus domain models.
//!
//! The WebSocket client maintains internal caches for order book reconstruction
//! and provides efficient parsing of BitMEX's table-based update format.

pub mod client;
pub mod enums;
pub mod error;
pub mod handler;
pub mod messages;
pub mod parse;

pub use crate::websocket::client::BitmexWebSocketClient;
