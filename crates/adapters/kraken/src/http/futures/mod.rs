//! HTTP/REST client implementation for Kraken Futures API.

pub mod client;
pub mod models;
pub mod query;

pub use client::KrakenFuturesHttpClient;
pub use query::*;
