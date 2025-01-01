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

use nautilus_core::python::to_pyvalue_err;
use pyo3::prelude::*;

use crate::signing::{ed25519_signature, hmac_signature, rsa_signature};

#[pyfunction(name = "hmac_signature")]
pub fn py_hmac_signature(secret: &str, data: &str) -> PyResult<String> {
    Ok(hmac_signature(secret, data))
}

#[pyfunction(name = "rsa_signature")]
pub fn py_rsa_signature(private_key_pem: &str, data: &str) -> PyResult<String> {
    rsa_signature(private_key_pem, data).map_err(to_pyvalue_err)
}

#[pyfunction(name = "ed25519_signature")]
pub fn py_ed25519_signature(private_key: &[u8], data: &str) -> PyResult<String> {
    ed25519_signature(private_key, data).map_err(to_pyvalue_err)
}
