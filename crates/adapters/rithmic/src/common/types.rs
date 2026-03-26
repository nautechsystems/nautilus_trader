//! Type aliases and common type definitions.

use std::sync::Arc;

/// Shared reference type for thread-safe access.
pub type SharedRef<T> = Arc<T>;

/// Rithmic symbol (e.g., "ESZ4" for December 2024 E-mini S&P).
pub type RithmicSymbol = String;

/// Exchange identifier (e.g., "CME").
pub type ExchangeId = String;

/// Rithmic account identifier.
pub type RithmicAccountId = String;

/// Rithmic order ID (assigned by venue).
pub type RithmicOrderId = String;

/// Client order ID (assigned locally).
pub type ClientOrderIdStr = String;

/// Unix timestamp in nanoseconds.
pub type UnixNanos = u64;

/// Price as a decimal value.
pub type PriceValue = f64;

/// Quantity as a decimal value.
pub type QuantityValue = f64;
