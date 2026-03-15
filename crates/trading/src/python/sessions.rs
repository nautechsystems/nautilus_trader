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

use std::str::FromStr;

use chrono::{DateTime, Utc};
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::python::common::EnumIterator;
use pyo3::{PyTypeInfo, prelude::*, types::PyType};

use crate::sessions::{
    ForexSession, fx_local_from_utc, fx_next_end, fx_next_start, fx_prev_end, fx_prev_start,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ForexSession {
    /// Represents a major Forex market session based on trading hours.
    #[new]
    fn py_new(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(&t, value)
    }

    const fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(PositionSide),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub const fn value(&self) -> u8 {
        *self as u8
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }
}

/// Converts a UTC timestamp to the local time for the given Forex session.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.trading")]
#[pyo3(name = "fx_local_from_utc")]
pub fn py_fx_local_from_utc(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<String> {
    Ok(fx_local_from_utc(session, time_now).to_rfc3339())
}

/// Returns the next session start time in UTC.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.trading")]
#[pyo3(name = "fx_next_start")]
pub fn py_fx_next_start(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_next_start(session, time_now))
}

/// Returns the next session end time in UTC.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.trading")]
#[pyo3(name = "fx_next_end")]
pub fn py_fx_next_end(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_next_end(session, time_now))
}

/// Returns the previous session start time in UTC.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.trading")]
#[pyo3(name = "fx_prev_start")]
pub fn py_fx_prev_start(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_prev_start(session, time_now))
}

/// Returns the previous session end time in UTC.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.trading")]
#[pyo3(name = "fx_prev_end")]
pub fn py_fx_prev_end(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_prev_end(session, time_now))
}
