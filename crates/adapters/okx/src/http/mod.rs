//! HTTP/REST client implementation for the OKX v5 API.
//!
//! This module provides a HTTP client for interacting with OKX's REST endpoints, including:
//!
//! - Market data queries (instruments, trades, bars, tickers).
//! - Account information and balances.
//! - Order management and execution.
//! - Position queries and management.
//! - Request signing and rate limiting.

pub mod client;
pub mod error;
pub mod models;
pub mod parse;
pub mod query;

// Re-exports
pub use crate::http::client::OKXHttpClient;
