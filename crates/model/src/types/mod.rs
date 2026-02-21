// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Value types for the trading domain model.
//!
//! This module provides immutable value types that represent fundamental trading concepts:
//! [`Price`], [`Quantity`], and [`Money`]. These types use fixed-point arithmetic internally
//! for deterministic calculations while providing a natural numeric interface.
//!
//! # Immutability
//!
//! All value types are **immutable** - once constructed, their values cannot change.
//! Arithmetic operations return new instances rather than modifying existing ones.
//! This design ensures thread safety and predictable behavior in concurrent trading systems.
//!
//! # Arithmetic operations
//!
//! Value types implement Rust's standard arithmetic traits (`Add`, `Sub`, `Mul`) for
//! same-type operations. When operating on two values of the same type, the result
//! preserves that type.
//!
//! | Operation             | Result     | Notes                                     |
//! |-----------------------|------------|-------------------------------------------|
//! | `Quantity + Quantity` | `Quantity` | Precision is max of both operands.        |
//! | `Quantity - Quantity` | `Quantity` | Panics if result would be negative.       |
//! | `Price + Price`       | `Price`    | Precision is max of both operands.        |
//! | `Price - Price`       | `Price`    | Precision is max of both operands.        |
//! | `Money + Money`       | `Money`    | Panics if currencies don't match.         |
//! | `Money - Money`       | `Money`    | Panics if currencies don't match.         |
//!
//! For Python bindings with mixed-type operations (e.g., `Quantity + int`), see the
//! Python API documentation.
//!
//! # Precision
//!
//! Each value type stores a precision field indicating the number of decimal places.
//! The maximum precision is defined by [`fixed::FIXED_PRECISION`]. When performing
//! arithmetic between values with different precisions, the result uses the maximum
//! precision of the operands.
//!
//! # Constraints
//!
//! - [`Quantity`]: Non-negative values only. Subtracting a larger quantity from a smaller
//!   one raises an error rather than producing a negative result.
//! - [`Price`]: Signed values allowed (can represent negative prices for spreads, etc.).
//! - [`Money`]: Signed values allowed. Operations between different currencies raise an error.

pub mod balance;
pub mod currency;
pub mod fixed;
pub mod money;
pub mod price;
pub mod quantity;

#[cfg(any(test, feature = "stubs"))]
pub mod stubs;

// Re-exports
pub use balance::{AccountBalance, MarginBalance};
pub use currency::Currency;
pub use money::{MONEY_MAX, MONEY_MIN, Money};
pub use price::{
    ERROR_PRICE, PRICE_ERROR, PRICE_MAX, PRICE_MIN, PRICE_RAW_MAX, PRICE_RAW_MIN, PRICE_UNDEF,
    Price,
};
pub use quantity::{QUANTITY_MAX, QUANTITY_MIN, QUANTITY_UNDEF, Quantity};
