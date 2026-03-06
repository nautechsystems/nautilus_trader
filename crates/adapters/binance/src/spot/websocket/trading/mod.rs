//! Binance Spot WebSocket API client with SBE (Simple Binary Encoding) support.
//!
//! This module provides a WebSocket API client for Binance Spot trading using
//! the request/response pattern with SBE-encoded responses. It complements the
//! HTTP client by offering lower-latency order submission via WebSocket.
//!
//! ## Architecture
//!
//! The client uses a two-tier architecture per the adapter design guidelines:
//!
//! - **Outer client** (`BinanceSpotWsTradingClient`): Orchestrates connection lifecycle,
//!   maintains state for Python access, sends commands via channel.
//! - **Inner handler** (`BinanceSpotWsApiHandler`): Runs in dedicated Tokio task,
//!   owns WebSocket connection, processes commands and responses.
//!
//! ## Features
//!
//! - Order placement, cancellation, and modification via WebSocket
//! - SBE-encoded responses for consistency with HTTP client
//! - Ed25519 authentication
//! - Request/response correlation by ID

pub mod client;
pub mod error;
pub mod handler;
pub mod messages;

pub use client::BinanceSpotWsTradingClient;
pub use error::{BinanceWsApiError, BinanceWsApiResult};
pub use handler::BinanceSpotWsApiHandler;
pub use messages::{HandlerCommand, NautilusWsApiMessage};
