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

use nautilus_model::data::Bar;
use pyo3::prelude::*;

use crate::{indicator::Indicator, momentum::stochastics::Stochastics};

#[pymethods]
impl Stochastics {
    #[new]
    #[must_use]
    pub fn py_new(period_k: usize, period_d: usize) -> Self {
        Self::new(period_k, period_d)
    }

    fn __repr__(&self) -> String {
        format!("Stochastics({},{})", self.period_k, self.period_d)
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "period_k")]
    const fn py_period_k(&self) -> usize {
        self.period_k
    }

    #[getter]
    #[pyo3(name = "period_d")]
    const fn py_period_d(&self) -> usize {
        self.period_d
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "value_k")]
    const fn py_value_k(&self) -> f64 {
        self.value_k
    }

    #[getter]
    #[pyo3(name = "value_d")]
    const fn py_value_d(&self) -> f64 {
        self.value_d
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, high: f64, low: f64, close: f64) {
        self.update_raw(high, low, close);
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) {
        self.handle_bar(bar);
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}
