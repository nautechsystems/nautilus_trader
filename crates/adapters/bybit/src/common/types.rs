//! Result types for Bybit margin operations.
//!
//! These types are used for strategy-level communication of margin operation results.

/// Result from a Bybit borrow operation for strategy consumption.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
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
