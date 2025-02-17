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

pub mod sessions;

use pyo3::{prelude::*, pymodule};

/// Loaded as nautilus_pyo3.trading
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn trading(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::sessions::ForexSession>()?;
    m.add_function(wrap_pyfunction!(sessions::py_fx_local_from_utc, m)?)?;
    m.add_function(wrap_pyfunction!(sessions::py_fx_next_start, m)?)?;
    m.add_function(wrap_pyfunction!(sessions::py_fx_prev_start, m)?)?;
    m.add_function(wrap_pyfunction!(sessions::py_fx_next_end, m)?)?;
    m.add_function(wrap_pyfunction!(sessions::py_fx_prev_end, m)?)?;
    Ok(())
}
