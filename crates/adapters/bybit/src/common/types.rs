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

//! Result types for Bybit margin operations.
//!
//! These types are used for strategy-level communication of margin operation results.

/// Result from a Bybit borrow operation for strategy consumption.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit")
)]
pub struct BybitMarginBorrowResult {
    /// The coin that was borrowed.
    pub coin: String,
    /// The amount that was borrowed.
    pub amount: String,
    /// Whether the borrow operation was successful.
    pub success: bool,
    /// Error message if the operation failed.
    pub message: String,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: u64,
    /// UNIX timestamp (nanoseconds) when the object was initialized.
    pub ts_init: u64,
}

/// Result from a Bybit repay operation for strategy consumption.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit")
)]
pub struct BybitMarginRepayResult {
    /// The coin that was repaid.
    pub coin: String,
    /// The amount that was repaid (None if repaying all).
    pub amount: Option<String>,
    /// Whether the repay operation was successful.
    pub success: bool,
    /// The result status from Bybit API.
    pub result_status: String,
    /// Error message if the operation failed.
    pub message: String,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: u64,
    /// UNIX timestamp (nanoseconds) when the object was initialized.
    pub ts_init: u64,
}

/// Result with current borrowed amount on Bybit.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit")
)]
pub struct BybitMarginStatusResult {
    /// The coin being queried.
    pub coin: String,
    /// The current borrowed amount.
    pub borrow_amount: String,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: u64,
    /// UNIX timestamp (nanoseconds) when the object was initialized.
    pub ts_init: u64,
}
