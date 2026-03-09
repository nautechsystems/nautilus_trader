//! Common types and utilities shared across the BitMEX adapter.
//!
//! This module provides reusable components that are used by both the HTTP and WebSocket
//! clients, including:
//! - Constants for BitMEX URLs and venue identifier.
//! - Credential management for API authentication.
//! - Enumerations for order types, sides, and statuses.
//! - Parsing utilities for currency codes and other data transformations.

pub mod consts;
pub mod credential;
pub mod enums;
pub mod parse;
pub mod retry;
pub mod urls;

#[cfg(test)]
pub(crate) mod testing;
