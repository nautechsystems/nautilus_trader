// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Value types for the trading domain model such as `Price`, `Quantity` and `Money`.

pub mod balance;
pub mod currency;
pub mod fixed;
pub mod money;
pub mod price;
pub mod quantity;

#[cfg(feature = "stubs")]
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
