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
