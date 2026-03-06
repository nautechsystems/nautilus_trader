//! Cap'n Proto serialization for Nautilus types.
//!
//! This module provides Cap'n Proto serialization support for Nautilus domain types.
//! The generated schema modules are available at the crate root for proper cross-referencing.
//!
//! # Generated Modules
//!
//! The following modules are generated from Cap'n Proto schemas:
//! - `crate::base_capnp` - Base types (UUID4, UnixNanos, StringMap)
//! - `crate::identifiers_capnp` - Identifier types
//! - `crate::types_capnp` - Value types (Price, Quantity, Money, etc.)
//! - `crate::enums_capnp` - Enumerations
//! - `crate::trading_capnp` - Trading commands
//! - `crate::data_capnp` - Data commands and responses
//! - `crate::order_capnp` - Order events
//! - `crate::position_capnp` - Position events
//! - `crate::account_capnp` - Account events
//! - `crate::market_capnp` - Market data types

pub mod conversions;

// Re-export generated modules for convenience.
// Re-export conversion functions for use by other crates
pub use conversions::order_side_to_capnp;

pub use crate::{
    account_capnp, base_capnp, data_capnp, enums_capnp, identifiers_capnp, market_capnp,
    order_capnp, position_capnp, trading_capnp, types_capnp,
};

/// Trait for converting Rust types to Cap'n Proto builders.
pub trait ToCapnp<'a> {
    /// The Cap'n Proto builder type for this Rust type.
    type Builder;

    /// Convert this Rust value to a Cap'n Proto builder.
    fn to_capnp(&self, builder: Self::Builder);
}

/// Trait for converting Cap'n Proto readers to Rust types.
pub trait FromCapnp<'a> {
    /// The Cap'n Proto reader type for this Rust type.
    type Reader;

    /// Convert a Cap'n Proto reader to this Rust type.
    ///
    /// # Errors
    ///
    /// Returns an error if the Cap'n Proto data is invalid or cannot be converted.
    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn std::error::Error>>
    where
        Self: Sized;
}
