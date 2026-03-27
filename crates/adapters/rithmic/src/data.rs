//! Rithmic data client for market data streaming.
//!
//! This module provides the data client that connects to Rithmic's
//! ticker plant for streaming quotes, trades, and market depth.

mod client;
mod handler;
mod parse;

pub use client::{
    MarketDataEvent, QuoteTick, RithmicBarType, RithmicDataClient, TimeBar, TradeTick,
};
