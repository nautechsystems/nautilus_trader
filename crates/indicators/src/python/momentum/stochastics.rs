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

use crate::{
    average::MovingAverageType,
    indicator::Indicator,
    momentum::stochastics::{Stochastics, StochasticsDMethod},
    python::float_precision,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl StochasticsDMethod {
    const fn __hash__(&self) -> isize {
        *self as isize
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Stochastics {
    /// Stochastic oscillator with smoothed K and D outputs.
    ///
    /// Defaults to `slowing = 1`, `ma_type = Simple`, and `d_method = MovingAverage`,
    /// so D is a simple moving average of K. Select `StochasticsDMethod.Ratio`
    /// for the legacy Nautilus range-weighted D calculation.
    #[new]
    #[pyo3(signature = (period_k, period_d, slowing=None, ma_type=None, d_method=None))]
    pub fn py_new(
        period_k: usize,
        period_d: usize,
        slowing: Option<usize>,
        ma_type: Option<MovingAverageType>,
        d_method: Option<StochasticsDMethod>,
    ) -> PyResult<Self> {
        Self::new_checked(
            period_k,
            period_d,
            slowing.unwrap_or(Self::DEFAULT_SLOWING),
            ma_type.unwrap_or(Self::DEFAULT_MA_TYPE),
            d_method.unwrap_or(Self::DEFAULT_D_METHOD),
        )
        .map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!(
            "Stochastics({},{},{},{:?},{:?})",
            self.period_k, self.period_d, self.slowing, self.ma_type, self.d_method
        )
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
    #[pyo3(name = "slowing")]
    const fn py_slowing(&self) -> usize {
        self.slowing
    }

    #[getter]
    #[pyo3(name = "ma_type")]
    const fn py_ma_type(&self) -> MovingAverageType {
        self.ma_type
    }

    #[getter]
    #[pyo3(name = "d_method")]
    const fn py_d_method(&self) -> StochasticsDMethod {
        self.d_method
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

    /// Updates the indicator with raw price values.
    ///
    /// # Parameters
    ///
    /// - `high`: The high price for the period.
    /// - `low`: The low price for the period.
    /// - `close`: The close price for the period.
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
