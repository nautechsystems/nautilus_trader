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

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::data::Bar;
use pyo3::prelude::*;

use crate::{indicator::Indicator, momentum::ichimoku::IchimokuCloud, python::float_precision};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IchimokuCloud {
    /// Ichimoku Kinko Hyo: the five-line cloud chart.
    ///
    /// ```text
    /// tenkan_sen    = midpoint(high, low over tenkan_period)
    /// kijun_sen     = midpoint(high, low over kijun_period)
    /// senkou_span_a = (tenkan_sen + kijun_sen) / 2      as computed `displacement - 1` bars ago
    /// senkou_span_b = midpoint(high, low over senkou_period) as computed `displacement - 1` bars ago
    /// chikou_span   = close from `displacement - 1` bars ago
    /// ```
    ///
    /// The two Senkou spans form the Kumo (cloud). Charts draw them `displacement`
    /// bars ahead and the Chikou span `displacement` bars behind; streaming in
    /// chronological order, the values visible at bar `n` are the ones buffered
    /// `displacement` updates ago, that is bar `n - displacement + 1`.
    ///
    /// Each line becomes available at its own bar: `tenkan_sen` after `tenkan_period`
    /// bars, `kijun_sen` after `kijun_period`, `chikou_span` after `displacement`,
    /// `senkou_span_a` after `kijun_period + displacement - 1`, and `senkou_span_b`
    /// after `senkou_period + displacement - 1` (77 bars at the classic
    /// `(9, 26, 52, 26)`). The matching `has_*` flag reports whether each field
    /// holds a value, and `initialized` gates on all five.
    #[new]
    #[pyo3(signature = (tenkan_period=9, kijun_period=26, senkou_period=52, displacement=26))]
    fn py_new(
        tenkan_period: usize,
        kijun_period: usize,
        senkou_period: usize,
        displacement: usize,
    ) -> PyResult<Self> {
        Self::new_checked(tenkan_period, kijun_period, senkou_period, displacement)
            .map_err(to_pyvalue_err)
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
    #[pyo3(name = "has_tenkan")]
    const fn py_has_tenkan(&self) -> bool {
        self.has_tenkan
    }

    #[getter]
    #[pyo3(name = "has_kijun")]
    const fn py_has_kijun(&self) -> bool {
        self.has_kijun
    }

    #[getter]
    #[pyo3(name = "has_senkou_a")]
    const fn py_has_senkou_a(&self) -> bool {
        self.has_senkou_a
    }

    #[getter]
    #[pyo3(name = "has_senkou_b")]
    const fn py_has_senkou_b(&self) -> bool {
        self.has_senkou_b
    }

    #[getter]
    #[pyo3(name = "has_chikou")]
    const fn py_has_chikou(&self) -> bool {
        self.has_chikou
    }

    #[getter]
    #[pyo3(name = "count")]
    const fn py_count(&self) -> usize {
        self.count
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    /// Updates the indicator with the given high, low and close.
    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, high: f64, low: f64, close: f64) {
        self.update_raw(high, low, close);
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) -> PyResult<()> {
        float_precision::check_bar(bar)?;
        self.handle_bar(bar);
        Ok(())
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}
