//! Binance Spot HTTP client with SBE encoding support.

pub mod client;
pub mod error;
pub mod models;
pub mod parse;
pub mod query;

pub use client::{BinanceRawSpotHttpClient, BinanceSpotHttpClient, SBE_SCHEMA_HEADER};
pub use error::{BinanceSpotHttpError, BinanceSpotHttpResult, SbeDecodeError};
pub use models::{BinanceDepth, BinancePriceLevel, BinanceTrade, BinanceTrades};
pub use query::{DepthParams, TradesParams};
