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

//! Python bindings for DeFi pool profiler.

use pyo3::prelude::*;

use crate::{
    defi::{Pool, pool_analysis::PoolProfiler},
    identifiers::InstrumentId,
};

#[pymethods]
impl PoolProfiler {
    #[getter]
    #[pyo3(name = "pool")]
    fn py_pool(&self) -> Pool {
        self.pool.as_ref().clone()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.pool.instrument_id
    }

    #[getter]
    #[pyo3(name = "is_initialized")]
    fn py_is_initialized(&self) -> bool {
        self.is_initialized
    }

    #[getter]
    #[pyo3(name = "current_tick")]
    fn py_current_tick(&self) -> i32 {
        self.state.current_tick
    }

    #[getter]
    #[pyo3(name = "price_sqrt_ratio_x96")]
    fn py_price_sqrt_ratio_x96(&self) -> String {
        self.state.price_sqrt_ratio_x96.to_string()
    }

    #[getter]
    #[pyo3(name = "total_amount0_deposited")]
    fn py_total_amount0_deposited(&self) -> String {
        self.analytics.total_amount0_deposited.to_string()
    }

    #[getter]
    #[pyo3(name = "total_amount1_deposited")]
    fn py_total_amount1_deposited(&self) -> String {
        self.analytics.total_amount1_deposited.to_string()
    }

    #[getter]
    #[pyo3(name = "total_amount0_collected")]
    fn py_total_amount0_collected(&self) -> String {
        self.analytics.total_amount0_collected.to_string()
    }

    #[getter]
    #[pyo3(name = "total_amount1_collected")]
    fn py_total_amount1_collected(&self) -> String {
        self.analytics.total_amount1_collected.to_string()
    }

    #[getter]
    #[pyo3(name = "protocol_fees_token0")]
    fn py_protocol_fees_token0(&self) -> String {
        self.state.protocol_fees_token0.to_string()
    }

    #[getter]
    #[pyo3(name = "protocol_fees_token1")]
    fn py_protocol_fees_token1(&self) -> String {
        self.state.protocol_fees_token1.to_string()
    }

    #[getter]
    #[pyo3(name = "fee_protocol")]
    fn py_fee_protocol(&self) -> u8 {
        self.state.fee_protocol
    }

    #[pyo3(name = "get_active_liquidity")]
    fn py_get_active_liquidity(&self) -> u128 {
        self.get_active_liquidity()
    }

    #[pyo3(name = "get_active_tick_count")]
    fn py_get_active_tick_count(&self) -> usize {
        self.get_active_tick_count()
    }

    #[pyo3(name = "get_total_tick_count")]
    fn py_get_total_tick_count(&self) -> usize {
        self.get_total_tick_count()
    }

    #[pyo3(name = "get_total_active_positions")]
    fn py_get_total_active_positions(&self) -> usize {
        self.get_total_active_positions()
    }

    #[pyo3(name = "get_total_inactive_positions")]
    fn py_get_total_inactive_positions(&self) -> usize {
        self.get_total_inactive_positions()
    }

    #[pyo3(name = "estimate_balance_of_token0")]
    fn py_estimate_balance_of_token0(&self) -> String {
        self.estimate_balance_of_token0().to_string()
    }

    #[pyo3(name = "estimate_balance_of_token1")]
    fn py_estimate_balance_of_token1(&self) -> String {
        self.estimate_balance_of_token1().to_string()
    }

    #[pyo3(name = "get_total_liquidity")]
    fn py_get_total_liquidity_all_positions(&self) -> String {
        self.get_total_liquidity().to_string()
    }

    #[pyo3(name = "liquidity_utilization_rate")]
    fn py_liquidity_utilization_rate(&self) -> f64 {
        self.liquidity_utilization_rate()
    }
}
