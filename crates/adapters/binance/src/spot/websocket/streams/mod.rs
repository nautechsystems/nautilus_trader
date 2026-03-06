//! Binance Spot WebSocket market data streams with SBE encoding.
//!
//! This module provides WebSocket connectivity to Binance's SBE market data streams
//! at `stream-sbe.binance.com`. These streams provide lower latency and smaller
//! payloads compared to JSON streams.
//!
//! ## Available Streams
//!
//! - `<symbol>@trade` - Real-time trade data
//! - `<symbol>@bestBidAsk` - Best bid/offer updates
//! - `<symbol>@depth` - Order book diff updates
//! - `<symbol>@depth20` - Top 20 order book levels
//!
//! ## Authentication
//!
//! SBE market data streams require Ed25519 API key authentication via the
//! `X-MBX-APIKEY` header.

pub mod client;
pub mod handler;
pub mod messages;
pub mod parse;
pub mod subscription;

pub use client::BinanceSpotWebSocketClient;
