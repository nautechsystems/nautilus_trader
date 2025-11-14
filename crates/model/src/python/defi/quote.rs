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

use pyo3::prelude::*;

use crate::defi::pool_analysis::quote::SwapQuote;

#[pymethods]
impl SwapQuote {
    #[getter]
    #[pyo3(name = "amount0")]
    fn py_amount0(&self) -> String {
        self.amount0.to_string()
    }

    #[getter]
    #[pyo3(name = "amount1")]
    fn py_amount1(&self) -> String {
        self.amount1.to_string()
    }

    #[getter]
    #[pyo3(name = "sqrt_price_before_x96")]
    fn py_sqrt_price_before_x96(&self) -> String {
        self.sqrt_price_before_x96.to_string()
    }

    #[getter]
    #[pyo3(name = "sqrt_price_after_x96")]
    fn py_sqrt_price_after_x96(&self) -> String {
        self.sqrt_price_after_x96.to_string()
    }

    #[getter]
    #[pyo3(name = "tick_before")]
    fn py_tick_before(&self) -> i32 {
        self.tick_before
    }

    #[getter]
    #[pyo3(name = "tick_after")]
    fn py_tick_after(&self) -> i32 {
        self.tick_after
    }

    #[getter]
    #[pyo3(name = "liquidity_after")]
    fn py_liquidity_after(&self) -> u128 {
        self.liquidity_after
    }

    #[getter]
    #[pyo3(name = "fee_growth_global_after")]
    fn py_fee_growth_global_after(&self) -> String {
        self.fee_growth_global_after.to_string()
    }

    #[getter]
    #[pyo3(name = "lp_fee")]
    fn py_lp_fee(&self) -> String {
        self.lp_fee.to_string()
    }

    #[getter]
    #[pyo3(name = "protocol_fee")]
    fn py_protocol_fee(&self) -> String {
        self.protocol_fee.to_string()
    }

    #[getter]
    #[pyo3(name = "crossed_ticks_count")]
    fn py_crossed_ticks_count(&self) -> usize {
        self.crossed_ticks.len()
    }

    #[pyo3(name = "zero_for_one")]
    fn py_zero_for_one(&self) -> bool {
        self.zero_for_one()
    }

    #[pyo3(name = "total_fee")]
    fn py_total_fee(&self) -> String {
        self.total_fee().to_string()
    }

    #[pyo3(name = "total_crossed_ticks")]
    fn py_total_crossed_ticks(&self) -> u32 {
        self.total_crossed_ticks()
    }

    #[pyo3(name = "get_output_amount")]
    fn py_get_output_amount(&self) -> String {
        self.get_output_amount().to_string()
    }

    fn __str__(&self) -> String {
        format!(
            "SwapQuote(amount0={}, amount1={}, tick_before={}, tick_after={}, liquidity_after={}, total_fee={})",
            self.amount0,
            self.amount1,
            self.tick_before,
            self.tick_after,
            self.liquidity_after,
            self.total_fee()
        )
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
