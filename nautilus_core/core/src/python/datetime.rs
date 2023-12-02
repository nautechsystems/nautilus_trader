// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use pyo3::prelude::*;

use crate::datetime::{
    micros_to_nanos, millis_to_nanos, nanos_to_micros, nanos_to_millis, nanos_to_secs,
    secs_to_millis, secs_to_nanos, unix_nanos_to_iso8601,
};

#[must_use]
#[pyfunction(name = "secs_to_nanos")]
pub fn py_secs_to_nanos(secs: f64) -> u64 {
    secs_to_nanos(secs)
}

#[must_use]
#[pyfunction(name = "secs_to_millis")]
pub fn py_secs_to_millis(secs: f64) -> u64 {
    secs_to_millis(secs)
}

#[must_use]
#[pyfunction(name = "millis_to_nanos")]
pub fn py_millis_to_nanos(millis: f64) -> u64 {
    millis_to_nanos(millis)
}

#[must_use]
#[pyfunction(name = "micros_to_nanos")]
pub fn py_micros_to_nanos(micros: f64) -> u64 {
    micros_to_nanos(micros)
}

#[must_use]
#[pyfunction(name = "nanos_to_secs")]
pub fn py_nanos_to_secs(nanos: u64) -> f64 {
    nanos_to_secs(nanos)
}

#[must_use]
#[pyfunction(name = "nanos_to_millis")]
pub fn py_nanos_to_millis(nanos: u64) -> u64 {
    nanos_to_millis(nanos)
}

#[must_use]
#[pyfunction(name = "nanos_to_micros")]
pub fn py_nanos_to_micros(nanos: u64) -> u64 {
    nanos_to_micros(nanos)
}

#[must_use]
#[pyfunction(name = "unix_nanos_to_iso8601")]
pub fn py_unix_nanos_to_iso8601(timestamp_ns: u64) -> String {
    unix_nanos_to_iso8601(timestamp_ns)
}
