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

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use pyo3::prelude::*;

use crate::sessions::{
    fx_local_from_utc, fx_next_end, fx_next_start, fx_prev_end, fx_prev_start, ForexSession,
};

/// Converts a UTC timestamp to the local time for the given Forex session.
#[pyfunction]
#[pyo3(name = "fx_local_from_utc")]
pub fn py_fx_local_from_utc(
    session: ForexSession,
    time_now: DateTime<Utc>,
) -> PyResult<DateTime<Tz>> {
    Ok(fx_local_from_utc(session, time_now))
}

/// Returns the next session start time in UTC.
#[pyfunction]
#[pyo3(name = "fx_next_start")]
pub fn py_fx_next_start(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_next_start(session, time_now))
}

/// Returns the previous session start time in UTC.
#[pyfunction]
#[pyo3(name = "fx_prev_start")]
pub fn py_fx_prev_start(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_prev_start(session, time_now))
}

/// Returns the next session end time in UTC.
#[pyfunction]
#[pyo3(name = "fx_next_end")]
pub fn py_fx_next_end(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_next_end(session, time_now))
}

/// Returns the previous session end time in UTC.
#[pyfunction]
#[pyo3(name = "fx_prev_end")]
pub fn py_fx_prev_end(session: ForexSession, time_now: DateTime<Utc>) -> PyResult<DateTime<Utc>> {
    Ok(fx_prev_end(session, time_now))
}
