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

use crate::{average::MovingAverageType, indicator::Indicator, momentum::swings::Swings};

#[pymethods]
impl Swings {
    #[new]
    pub fn py_new(period: usize, ma_type: Option<MovingAverageType>) -> Self {
        Self::new(period)
    }

    fn __repr__(&self) -> String {
        format!("Swings({})", self.period)
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "period")]
    fn py_period(&self) -> usize {
        self.period
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "direction")]
    fn py_direction(&self) -> i64 {
        self.direction
    }

    #[getter]
    #[pyo3(name = "changed")]
    fn py_changed(&self) -> bool {
        self.changed
    }

    #[getter]
    #[pyo3(name = "high_datetime")]
    fn py_high_datetime(&self) -> f64 {
        self.high_datetime
    }

    #[getter]
    #[pyo3(name = "low_datetime")]
    fn py_low_datetime(&self) -> f64 {
        self.low_datetime
    }

    #[getter]
    #[pyo3(name = "high_price")]
    fn py_high_price(&self) -> f64 {
        self.high_price
    }

    #[getter]
    #[pyo3(name = "low_price")]
    fn py_low_price(&self) -> f64 {
        self.low_price
    }

    #[getter]
    #[pyo3(name = "length")]
    fn py_length(&self) -> usize {
        self.length
    }

    #[getter]
    #[pyo3(name = "duration")]
    fn py_duration(&self) -> usize {
        self.duration
    }

    #[getter]
    #[pyo3(name = "since_high")]
    fn py_since_high(&self) -> usize {
        self.since_high
    }

    #[getter]
    #[pyo3(name = "since_low")]
    fn py_since_low(&self) -> usize {
        self.since_low
    }

    #[getter]
    #[pyo3(name = "initialized")]
    fn py_initialized(&self) -> bool {
        self.initialized()
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, high: f64, low: f64, timestamp: f64) {
        self.update_raw(high, low, timestamp);
    }

    #[pyo3(name = "handle_quote_tick")]
    fn py_handle_quote_tick(&mut self, _tick: &QuoteTick) {
        // Function body intentionally left blank.
    }

    #[pyo3(name = "handle_trade_tick")]
    fn py_handle_trade_tick(&mut self, _tick: &TradeTick) {
        // Function body intentionally left blank.
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.high).into(), (&bar.low).into(), bar.ts_init.as_f64());
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}
