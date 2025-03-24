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

//! Python bindings from [PyO3](https://pyo3.rs).

pub mod signing;

use pyo3::prelude::*;

use crate::python;

/// Loaded as nautilus_pyo3.cryptography
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn cryptography(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(python::signing::py_hmac_signature, m)?)?;
    m.add_function(wrap_pyfunction!(python::signing::py_rsa_signature, m)?)?;
    m.add_function(wrap_pyfunction!(python::signing::py_ed25519_signature, m)?)?;
    Ok(())
}
