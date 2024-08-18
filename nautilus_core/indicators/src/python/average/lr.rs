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
use pyo3::{exceptions::PyPermissionError, prelude::*};

use crate::{
    average::{lr::LinearRegression, MovingAverageType},
    indicator::Indicator,
};

#[pymethods]
impl LinearRegression {
    #[new]
    pub fn py_new(period: usize) -> Self {
        Self::new(period)
    }

    fn __repr__(&self) -> String {
        format!("LinearRegression({})", self.period)
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
    #[pyo3(name = "slope")]
    fn py_slope(&self) -> f64 {
        self.slope
    }

    #[getter]
    #[pyo3(name = "intercept")]
    fn py_intercept(&self) -> f64 {
        self.intercept
    }

    #[getter]
    #[pyo3(name = "degree")]
    fn py_degree(&self) -> f64 {
        self.degree
    }

    #[getter]
    #[pyo3(name = "cfo")]
    fn py_cfo(&self) -> f64 {
        self.cfo
    }

    #[getter]
    #[pyo3(name = "r2")]
    fn py_r2(&self) -> f64 {
        self.r2
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
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
    fn py_update_raw(&mut self, close: f64) {
        self.update_raw(close);
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
