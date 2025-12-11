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

use crate::{
    average::MovingAverageType,
    indicator::Indicator,
    momentum::stochastics::{Stochastics, StochasticsDMethod},
};

#[pymethods]
impl Stochastics {
    /// Creates a new Stochastics indicator.
    ///
    /// Parameters
    /// ----------
    /// period_k : int
    ///     The lookback period for %K calculation (highest high / lowest low).
    /// period_d : int
    ///     The smoothing period for %D calculation.
    /// slowing : int, optional
    ///     The slowing period for %K smoothing. Default is 1 (no slowing).
    ///     Use >1 for MA smoothed %K.
    /// ma_type : MovingAverageType, optional
    ///     The MA type for slowing and MA-based %D. Default is Exponential.
    /// d_method : StochasticsDMethod, optional
    ///     The %D calculation method. Default is Ratio (Nautilus original).
    ///     Use MovingAverage for MA smoothed %D.
    #[new]
    #[pyo3(signature = (period_k, period_d, slowing=None, ma_type=None, d_method=None))]
    #[must_use]
    pub fn py_new(
        period_k: usize,
        period_d: usize,
        slowing: Option<usize>,
        ma_type: Option<MovingAverageType>,
        d_method: Option<StochasticsDMethod>,
    ) -> Self {
        Self::new_with_params(
            period_k,
            period_d,
            slowing.unwrap_or(1),
            ma_type.unwrap_or(MovingAverageType::Exponential),
            d_method.unwrap_or(StochasticsDMethod::Ratio),
        )
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
