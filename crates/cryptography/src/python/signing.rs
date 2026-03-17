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
use pyo3::prelude::*;

use crate::signing::{ed25519_signature, hmac_signature, rsa_signature};

/// Generates an HMAC-SHA256 signature for the given data using the provided secret.
///
/// This function creates a cryptographic hash-based message authentication code (HMAC)
/// using SHA-256 as the underlying hash function. The resulting signature is returned
/// as a lowercase hexadecimal string.
///
/// # Errors
///
/// Returns an error if signature generation fails due to key or cryptographic errors.
#[pyfunction(name = "hmac_signature")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.cryptography")]
pub fn py_hmac_signature(secret: &str, data: &str) -> PyResult<String> {
    hmac_signature(secret, data).map_err(to_pyvalue_err)
}

/// Signs `data` using RSA PKCS#1 v1.5 SHA-256 with the provided private key in PEM format.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty.
/// - `private_key_pem` is not a valid PEM-encoded PKCS#8 RSA private key or cannot be parsed.
/// - Signature generation fails due to key or cryptographic errors.
#[pyfunction(name = "rsa_signature")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.cryptography")]
pub fn py_rsa_signature(private_key_pem: &str, data: &str) -> PyResult<String> {
    rsa_signature(private_key_pem, data).map_err(to_pyvalue_err)
}

/// Signs `data` using Ed25519 with the provided private key seed.
#[pyfunction(name = "ed25519_signature")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.cryptography")]
pub fn py_ed25519_signature(
    #[gen_stub(override_type(type_repr = "bytes"))] private_key: &[u8],
    data: &str,
) -> PyResult<String> {
    ed25519_signature(private_key, data).map_err(to_pyvalue_err)
}
