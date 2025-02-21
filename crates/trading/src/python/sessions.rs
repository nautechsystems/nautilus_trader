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

use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::python::common::EnumIterator;
use pyo3::{PyTypeInfo, prelude::*, types::PyType};

use crate::sessions::{
    ForexSession, fx_local_from_utc, fx_next_end, fx_next_start, fx_prev_end, fx_prev_start,
};

#[pymethods]
impl ForexSession {
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

    #[classattr]
    #[pyo3(name = "SYDNEY")]
    const fn py_no_position_side() -> Self {
        Self::Sydney
    }

    #[classattr]
    #[pyo3(name = "TOKYO")]
    const fn py_flat() -> Self {
        Self::Tokyo
    }

    #[classattr]
    #[pyo3(name = "LONDON")]
    const fn py_long() -> Self {
        Self::London
    }

    #[classattr]
    #[pyo3(name = "NEW_YORK")]
    const fn py_short() -> Self {
        Self::NewYork
    }
}

/// Converts a UTC timestamp to the local time for the given Forex session.
///
/// The `time_now` must be timezone-aware with its tzinfo set to a built-in `datetime.timezone`
/// (e.g. `datetime.timezone.utc`). Third-party tzinfo objects (like those from `pytz`) are not supported.
#[pyfunction]
#[pyo3(name = "fx_local_from_utc")]
pub fn py_fx_local_from_utc(
    session: ForexSession,
    time_now: DateTime<Utc>,
) -> PyResult<DateTime<Tz>> {
    Ok(fx_local_from_utc(session, time_now))
}

/// Returns the next session start time in UTC.
///
/// The `time_now` must be timezone-aware with its tzinfo set to a built-in `datetime.timezone`
/// (e.g. `datetime.timezone.utc`). Third-party tzinfo objects (like those from `pytz`) are not supported.
#[pyfunction]
#[pyo3(name = "fx_next_start")]
pub fn py_fx_next_start(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_next_start(session, time_now))
}

/// Returns the next session end time in UTC.
///
/// The `time_now` must be timezone-aware with its tzinfo set to a built-in `datetime.timezone`
/// (e.g. `datetime.timezone.utc`). Third-party tzinfo objects (like those from `pytz`) are not supported.
#[pyfunction]
#[pyo3(name = "fx_next_end")]
pub fn py_fx_next_end(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_next_end(session, time_now))
}

/// Returns the previous session start time in UTC.
///
/// The `time_now` must be timezone-aware with its tzinfo set to a built-in `datetime.timezone`
/// (e.g. `datetime.timezone.utc`). Third-party tzinfo objects (like those from `pytz`) are not supported.
#[pyfunction]
#[pyo3(name = "fx_prev_start")]
pub fn py_fx_prev_start(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_prev_start(session, time_now))
}

/// Returns the previous session end time in UTC.
///
/// The `time_now` must be timezone-aware with its tzinfo set to a built-in `datetime.timezone`
/// (e.g. `datetime.timezone.utc`). Third-party tzinfo objects (like those from `pytz`) are not supported.
#[pyfunction]
#[pyo3(name = "fx_prev_end")]
pub fn py_fx_prev_end(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_prev_end(session, time_now))
}
