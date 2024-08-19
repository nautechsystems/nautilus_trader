// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
use nautilus_model::data::{bar::Bar, quote::QuoteTick, trade::TradeTick};
use pyo3::prelude::*;

use crate::{average::MovingAverageType, indicator::Indicator, volatility::vr::VolatilityRatio};

#[pymethods]
impl VolatilityRatio {
    #[new]
    pub fn py_new(
        fast_period: usize,
        slow_period: usize,
        use_previous: Option<bool>,
        value_floor: Option<f64>,
        ma_type: Option<MovingAverageType>,
    ) -> Self {
        Self::new(fast_period, slow_period, ma_type, use_previous, value_floor)
    }

    fn __repr__(&self) -> String {
        format!("VolatilityRatio({},{})", self.fast_period, self.slow_period)
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "fast_period")]
    fn py_fast_period(&self) -> usize {
        self.fast_period
    }

    #[getter]
    #[pyo3(name = "slow_period")]
    fn py_slow_period(&self) -> usize {
        self.slow_period
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "use_previous")]
    fn py_use_previous(&self) -> bool {
        self.use_previous
    }

    #[getter]
    #[pyo3(name = "value_floor")]
    fn py_value_floor(&self) -> f64 {
        self.value_floor
    }

    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "initialized")]
    fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, high: f64, low: f64, close: f64) {
        self.update_raw(high, low, close);
    }

    #[pyo3(name = "handle_quote_tick")]
    fn py_handle_quote_tick(&mut self, tick: &QuoteTick) {
        // Function body intentionally left blank.
    }

    #[pyo3(name = "handle_trade_tick")]
    fn py_handle_trade_tick(&mut self, tick: &TradeTick) {
        // Function body intentionally left blank.
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
