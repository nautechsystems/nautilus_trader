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
    average::MovingAverageType, indicator::Indicator, python::float_precision,
    volatility::kc::KeltnerChannel,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl KeltnerChannel {
    /// Keltner channel.
    #[new]
    #[pyo3(signature = (period, k_multiplier, atr_period=None, ma_type=None, ma_type_atr=None, use_previous=None, atr_floor=None))]
    pub fn py_new(
        period: usize,
        k_multiplier: f64,
        atr_period: Option<usize>,
        ma_type: Option<MovingAverageType>,
        ma_type_atr: Option<MovingAverageType>,
        use_previous: Option<bool>,
        atr_floor: Option<f64>,
    ) -> PyResult<Self> {
        Self::new_checked(
            period,
            k_multiplier,
            atr_period,
            ma_type,
            ma_type_atr,
            use_previous,
            atr_floor,
        )
        .map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!("KeltnerChannel({})", self.period)
    }

    #[getter]
    #[pyo3(name = "ma_type")]
    const fn py_ma_type(&self) -> MovingAverageType {
        self.ma_type
    }

    #[getter]
    #[pyo3(name = "ma_type_atr")]
    const fn py_ma_type_atr(&self) -> MovingAverageType {
        self.ma_type_atr
    }

    #[getter]
    #[pyo3(name = "atr_period")]
    const fn py_atr_period(&self) -> usize {
        self.atr_period
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "period")]
    const fn py_period(&self) -> usize {
        self.period
    }

    #[getter]
    #[pyo3(name = "k_multiplier")]
    const fn py_k_multiplier(&self) -> f64 {
        self.k_multiplier
    }

    #[getter]
    #[pyo3(name = "use_previous")]
    const fn py_use_previous(&self) -> bool {
        self.use_previous
    }

    #[getter]
    #[pyo3(name = "atr_floor")]
    const fn py_atr_floor(&self) -> f64 {
        self.atr_floor
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "upper")]
    const fn py_upper(&self) -> f64 {
        self.upper
    }

    #[getter]
    #[pyo3(name = "middle")]
    const fn py_middle(&self) -> f64 {
        self.middle
    }

    #[getter]
    #[pyo3(name = "lower")]
    const fn py_lower(&self) -> f64 {
        self.lower
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
