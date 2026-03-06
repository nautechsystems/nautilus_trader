//! Common functionality shared across the dYdX adapter.
//!
//! This module provides core utilities, constants, and data structures used throughout
//! the dYdX integration, including:
//!
//! - **Authentication**: Wallet credential storage and message signing for dYdX v4.
//! - **Common enumerations**: dYdX-specific enums mirrored from REST/WebSocket payloads.
//! - **Constants**: Venue identifiers, broker IDs, and scaling factors.
//! - **Parsing utilities**: Helpers for converting dYdX data to Nautilus types.
//! - **URL management**: Environment-aware base URL resolvers for testnet and mainnet.
//! - **Common models**: Shared data structures used across HTTP and WebSocket layers.
//! - **Testing helpers**: Fixtures and utilities for unit tests.

pub mod consts;
pub mod credential;
pub mod enums;
pub mod instrument_cache;
pub mod models;
pub mod parse;
pub mod urls;

#[cfg(test)]
pub(crate) mod testing;
