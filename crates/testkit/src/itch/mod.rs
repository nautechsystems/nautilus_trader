//! NASDAQ TotalView-ITCH 5.0 parsing utilities for test data curation.
//!
//! Converts raw ITCH binary data into NautilusTrader
//! [`OrderBookDelta`](nautilus_model::data::delta::OrderBookDelta) events
//! for use in order book and matching engine tests.

pub mod parse;
