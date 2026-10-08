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

use crate::{indicator::Indicator, momentum::dm::DirectionalMovement, python::float_precision};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DirectionalMovement {
    /// Wilder's directional movement, smoothed as a running sum.
    ///
    /// The up-move is `high - previous_high` and the down-move is
    /// `previous_low - low`; only the larger of the two contributes, and only when it
    /// is positive. The first `period` movements, which start with the second bar,
    /// are summed to seed `pos` and `neg`. Later bars update them with
    /// `smoothed = smoothed - smoothed / period + movement`, so both values are
    /// on the scale of a `period`-bar sum of movements. The first complete output
    /// arrives after `period + 1` bars.
    #[new]
    fn py_new(period: usize) -> PyResult<Self> {
        Self::new_checked(period).map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!("DirectionalMovement({})", self.period)
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
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "pos")]
    const fn py_pos(&self) -> f64 {
        self.pos
    }

    #[getter]
    #[pyo3(name = "neg")]
    const fn py_neg(&self) -> f64 {
        self.neg
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, high: f64, low: f64) {
        self.update_raw(high, low);
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
