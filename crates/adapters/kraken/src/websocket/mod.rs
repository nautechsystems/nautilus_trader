//! WebSocket client implementations for Kraken APIs.
//!
//! This module provides WebSocket clients for real-time market data streams:
//!
//! - [`spot_v2`]: Kraken Spot WebSocket v2 API
//! - [`futures`]: Kraken Futures WebSocket v1 API

pub mod error;
pub mod futures;
pub mod spot_v2;
