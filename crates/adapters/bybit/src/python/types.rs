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

//! Python bindings for Bybit margin data types.

use pyo3::prelude::*;

use crate::common::types::{
    BybitMarginBorrowResult, BybitMarginRepayResult, BybitMarginStatusResult,
};

#[pymethods]
impl BybitMarginBorrowResult {
    #[new]
    #[must_use]
    pub fn py_new(
        coin: String,
        amount: String,
        success: bool,
        message: String,
        ts_event: u64,
        ts_init: u64,
    ) -> Self {
        Self {
            coin,
            amount,
            success,
            message,
            ts_event,
            ts_init,
        }
    }

    #[getter]
    #[must_use]
    pub fn coin(&self) -> &str {
        &self.coin
    }

    #[getter]
    #[must_use]
    pub fn amount(&self) -> &str {
        &self.amount
    }

    #[getter]
    #[must_use]
    pub fn success(&self) -> bool {
        self.success
    }

    #[getter]
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[getter]
    #[must_use]
    pub fn ts_event(&self) -> u64 {
        self.ts_event
    }

    #[getter]
    #[must_use]
    pub fn ts_init(&self) -> u64 {
        self.ts_init
    }

    fn __repr__(&self) -> String {
        format!(
            "BybitMarginBorrowResult(coin='{}', amount='{}', success={}, message='{}')",
            self.coin, self.amount, self.success, self.message
        )
    }
}

#[pymethods]
impl BybitMarginRepayResult {
    #[new]
    #[pyo3(signature = (coin, amount, success, result_status, message, ts_event, ts_init))]
    #[must_use]
    pub fn py_new(
        coin: String,
        amount: Option<String>,
        success: bool,
        result_status: String,
        message: String,
        ts_event: u64,
        ts_init: u64,
    ) -> Self {
        Self {
            coin,
            amount,
            success,
            result_status,
            message,
            ts_event,
            ts_init,
        }
    }

    #[getter]
    #[must_use]
    pub fn coin(&self) -> &str {
        &self.coin
    }

    #[getter]
    #[must_use]
    pub fn amount(&self) -> Option<&str> {
        self.amount.as_deref()
    }

    #[getter]
    #[must_use]
    pub fn success(&self) -> bool {
        self.success
    }

    #[getter]
    #[must_use]
    pub fn result_status(&self) -> &str {
        &self.result_status
    }

    #[getter]
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[getter]
    #[must_use]
    pub fn ts_event(&self) -> u64 {
        self.ts_event
    }

    #[getter]
    #[must_use]
    pub fn ts_init(&self) -> u64 {
        self.ts_init
    }

    fn __repr__(&self) -> String {
        format!(
            "BybitMarginRepayResult(coin='{}', success={}, result_status='{}')",
            self.coin, self.success, self.result_status
        )
    }
}

#[pymethods]
impl BybitMarginStatusResult {
    #[new]
    #[must_use]
    pub fn py_new(coin: String, borrow_amount: String, ts_event: u64, ts_init: u64) -> Self {
        Self {
            coin,
            borrow_amount,
            ts_event,
            ts_init,
        }
    }

    #[getter]
    #[must_use]
    pub fn coin(&self) -> &str {
        &self.coin
    }

    #[getter]
    #[must_use]
    pub fn borrow_amount(&self) -> &str {
        &self.borrow_amount
    }

    #[getter]
    #[must_use]
    pub fn ts_event(&self) -> u64 {
        self.ts_event
    }

    #[getter]
    #[must_use]
    pub fn ts_init(&self) -> u64 {
        self.ts_init
    }

    fn __repr__(&self) -> String {
        format!(
            "BybitMarginStatusResult(coin='{}', borrow_amount='{}')",
            self.coin, self.borrow_amount
        )
    }
}
