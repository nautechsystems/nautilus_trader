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

//! Python bindings for the [`BacktestResult`] type.

use std::collections::HashMap;

use nautilus_core::UUID4;

use crate::result::BacktestResult;

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl BacktestResult {
    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> &str {
        &self.trader_id
    }

    #[getter]
    #[pyo3(name = "machine_id")]
    fn py_machine_id(&self) -> &str {
        &self.machine_id
    }

    #[getter]
    #[pyo3(name = "instance_id")]
    const fn py_instance_id(&self) -> UUID4 {
        self.instance_id
    }

    #[getter]
    #[pyo3(name = "run_config_id")]
    fn py_run_config_id(&self) -> Option<&str> {
        self.run_config_id.as_deref()
    }

    #[getter]
    #[pyo3(name = "elapsed_time_secs")]
    const fn py_elapsed_time_secs(&self) -> f64 {
        self.elapsed_time_secs
    }

    #[getter]
    #[pyo3(name = "iterations")]
    const fn py_iterations(&self) -> usize {
        self.iterations
    }

    #[getter]
    #[pyo3(name = "total_events")]
    const fn py_total_events(&self) -> usize {
        self.total_events
    }

    #[getter]
    #[pyo3(name = "total_orders")]
    const fn py_total_orders(&self) -> usize {
        self.total_orders
    }

    #[getter]
    #[pyo3(name = "total_positions")]
    const fn py_total_positions(&self) -> usize {
        self.total_positions
    }

    #[getter]
    #[pyo3(name = "stats_pnls")]
    fn py_stats_pnls(&self) -> HashMap<String, HashMap<String, f64>> {
        self.stats_pnls
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.iter().map(|(k2, v2)| (k2.clone(), *v2)).collect(),
                )
            })
            .collect()
    }

    #[getter]
    #[pyo3(name = "stats_returns")]
    fn py_stats_returns(&self) -> HashMap<String, f64> {
        self.stats_returns
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    #[getter]
    #[pyo3(name = "stats_general")]
    fn py_stats_general(&self) -> HashMap<String, f64> {
        self.stats_general
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "BacktestResult(trader_id='{}', elapsed={:.2}s, iterations={}, orders={}, positions={})",
            self.trader_id,
            self.elapsed_time_secs,
            self.iterations,
            self.total_orders,
            self.total_positions,
        )
    }
}
