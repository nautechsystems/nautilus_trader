//! Binance SBE (Simple Binary Encoding) codec implementations.
//!
//! This module contains:
//! - `cursor`: Re-export of shared cursor utilities from `nautilus_serialization::sbe`.
//! - `error`: Re-export of shared decode error types from `nautilus_serialization::sbe`.
//! - `spot`: Generated codecs for the Spot REST/WebSocket API (schema 3:2).
//! - `stream`: Hand-written codecs for market data streams (schema 1:0).
//!
//! The spot codecs are generated from Binance's official SBE schema using
//! Real Logic's SBE generator. The stream codecs are hand-written for the
//! 4 market data stream message types.

pub mod cursor;
pub mod error;
pub mod spot;
pub mod stream;

pub use cursor::SbeCursor;
pub use error::{MAX_GROUP_SIZE, SbeDecodeError};
pub use spot::{
    ReadBuf, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION, SbeErr, SbeResult,
    message_header_codec::MessageHeaderDecoder,
};
