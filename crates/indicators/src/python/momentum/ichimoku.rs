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

use nautilus_model::data::Bar;
use pyo3::prelude::*;

use crate::{indicator::Indicator, momentum::ichimoku::IchimokuCloud};

#[pymethods]
impl IchimokuCloud {
    #[new]
    #[pyo3(signature = (tenkan_period=9, kijun_period=26, senkou_period=52, displacement=26))]
    #[must_use]
    pub fn py_new(
        tenkan_period: usize,
        kijun_period: usize,
        senkou_period: usize,
        displacement: usize,
    ) -> Self {
        Self::new(tenkan_period, kijun_period, senkou_period, displacement)
    }

    fn __repr__(&self) -> String {
        format!("{self}")
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "tenkan_period")]
    const fn py_tenkan_period(&self) -> usize {
        self.tenkan_period
    }

    #[getter]
    #[pyo3(name = "kijun_period")]
    const fn py_kijun_period(&self) -> usize {
        self.kijun_period
    }

    #[getter]
    #[pyo3(name = "senkou_period")]
    const fn py_senkou_period(&self) -> usize {
        self.senkou_period
    }

    #[getter]
    #[pyo3(name = "displacement")]
    const fn py_displacement(&self) -> usize {
        self.displacement
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "tenkan_sen")]
    const fn py_tenkan_sen(&self) -> f64 {
        self.tenkan_sen
    }

    #[getter]
    #[pyo3(name = "kijun_sen")]
    const fn py_kijun_sen(&self) -> f64 {
        self.kijun_sen
    }

    #[getter]
    #[pyo3(name = "senkou_span_a")]
    const fn py_senkou_span_a(&self) -> f64 {
        self.senkou_span_a
    }

    #[getter]
    #[pyo3(name = "senkou_span_b")]
    const fn py_senkou_span_b(&self) -> f64 {
        self.senkou_span_b
    }

    #[getter]
    #[pyo3(name = "chikou_span")]
    const fn py_chikou_span(&self) -> f64 {
        self.chikou_span
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
