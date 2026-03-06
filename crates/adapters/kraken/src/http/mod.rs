//! HTTP/REST client implementations for Kraken APIs.
//!
//! This module provides HTTP clients for interacting with Kraken's REST endpoints:
//!
//! - [`spot`]: Kraken Spot REST API
//! - [`futures`]: Kraken Futures REST API

pub mod error;
pub mod futures;
pub mod models;
pub mod spot;

// Re-exports
pub use error::KrakenHttpError;
pub use futures::{
    client::{KrakenFuturesHttpClient, KrakenFuturesRawHttpClient},
    query::*,
};
pub use spot::{
    client::{KrakenSpotHttpClient, KrakenSpotRawHttpClient},
    query::*,
};
