//! Common functionality shared across the OKX adapter.
//!
//! This module provides core utilities, constants, and data structures used throughout
//! the OKX integration, including:
//!
//! - Authentication credentials and signing.
//! - Common enumerations and constants.
//! - Parsing utilities for converting OKX data to Nautilus types.
//! - URL management for different endpoints.
//! - Shared data models.

pub mod consts;
pub mod credential;
pub mod enums;
pub mod models;
pub mod parse;
pub mod urls;

#[cfg(test)]
pub(crate) mod testing;
