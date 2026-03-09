//! HTTP/REST client implementation for Kraken Spot API.

pub mod client;
pub mod models;
pub mod query;

pub use client::KrakenSpotHttpClient;
pub use query::*;
