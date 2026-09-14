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
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};
use pyo3::prelude::*;

use crate::{average::zscore::ZScore, indicator::Indicator};

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl ZScore {
    /// Creates a new `ZScore` instance.
    ///
    /// Computes `(x - mean) / std` using sample standard deviation. The window
    /// expands until `period` observations, then rolls at that length. With one
    /// observation or a finite constant window, `mean` matches the input exactly,
    /// while `std` and `value` are 0. Other zero `std` values produce `value` 0;
    /// non-finite `std` values produce `value` `NaN`. `price_type` affects only
    /// quote handling.
    #[new]
    #[pyo3(signature = (period, price_type=None))]
    fn py_new(period: i64, price_type: Option<PriceType>) -> PyResult<Self> {
        if period < 0 {
            return Err(to_pyvalue_err("`period` must be at least 2"));
        }

        let period = usize::try_from(period).map_err(to_pyvalue_err)?;
        Self::new_checked(period, price_type).map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!("ZScore({})", self.period)
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
    #[pyo3(name = "price_type")]
    const fn py_price_type(&self) -> PriceType {
        self.price_type
    }

    #[getter]
    #[pyo3(name = "count")]
    const fn py_count(&self) -> usize {
        self.count
    }

    #[getter]
    #[pyo3(name = "value")]
    const fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "mean")]
    const fn py_mean(&self) -> f64 {
        self.mean
    }

    #[getter]
    #[pyo3(name = "std")]
    const fn py_std(&self) -> f64 {
        self.std
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "handle_quote_tick")]
    fn py_handle_quote_tick(&mut self, quote: &QuoteTick) -> PyResult<()> {
        self.handle_quote(quote).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "handle_trade_tick")]
    fn py_handle_trade_tick(&mut self, trade: &TradeTick) {
        self.handle_trade(trade);
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) {
        self.handle_bar(bar);
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    /// Updates the indicator with a raw observation.
    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, value: f64) {
        self.update_raw(value);
    }
}
