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
use nautilus_model::{
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
    enums::PriceType,
};
use pyo3::prelude::*;

use crate::{
    average::rma::WilderMovingAverage,
    indicator::{Indicator, MovingAverage},
};

#[pymethods]
impl WilderMovingAverage {
    #[new]
    fn py_new(period: usize, price_type: Option<PriceType>) -> PyResult<Self> {
        Self::new(period, price_type).map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!("WilderMovingAverage({})", self.period)
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
    #[pyo3(name = "alpha")]
    fn py_alpha(&self) -> f64 {
        self.alpha
    }

    #[getter]
    #[pyo3(name = "count")]
    fn py_count(&self) -> usize {
        self.count
    }

    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "initialized")]
    fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "handle_quote_tick")]
    fn py_handle_quote_tick(&mut self, tick: &QuoteTick) {
        self.py_update_raw(tick.extract_price(self.price_type).into());
    }

    #[pyo3(name = "handle_trade_tick")]
    fn py_handle_trade_tick(&mut self, tick: &TradeTick) {
        self.update_raw((&tick.price).into());
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, value: f64) {
        self.update_raw(value);
    }
}
