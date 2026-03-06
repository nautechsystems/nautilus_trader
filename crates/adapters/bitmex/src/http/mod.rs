//! HTTP REST API client implementation for BitMEX.
//!
//! This module provides an HTTP client for interacting with the BitMEX REST API.
//! It handles:
//! - Request signing and authentication.
//! - Rate limiting and retry logic.
//! - Request/response models.
//! - Parsing BitMEX data into Nautilus domain models.
//!
//! The client supports all major BitMEX REST endpoints including:
//! - Market data (instruments, trades, order books).
//! - Account data (wallet, positions, margins).
//! - Order management (place, modify, cancel orders).
//! - Execution history.

pub mod client;
pub mod error;
pub mod models;
pub mod parse;
pub mod query;
