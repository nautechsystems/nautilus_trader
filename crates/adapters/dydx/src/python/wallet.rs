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

//! Python bindings for dYdX wallet.

#![allow(clippy::missing_errors_doc)]

use std::sync::Arc;

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use pyo3::prelude::*;

use crate::execution::wallet::Wallet;

/// Python wrapper for the Wallet.
#[pyclass(name = "DydxWallet", from_py_object)]
#[derive(Debug, Clone)]
pub struct PyDydxWallet {
    pub(crate) inner: Arc<Wallet>,
}

#[pymethods]
impl PyDydxWallet {
    /// Create a wallet from a hex-encoded private key.
    ///
    /// # Errors
    ///
    /// Returns an error if the private key is invalid.
    #[staticmethod]
    #[pyo3(name = "from_private_key")]
    pub fn py_from_private_key(private_key: &str) -> PyResult<Self> {
        let wallet = Wallet::from_private_key(private_key).map_err(to_pyvalue_err)?;
        Ok(Self {
            inner: Arc::new(wallet),
        })
    }

    /// Get the wallet address.
    ///
    /// # Errors
    ///
    /// Returns an error if address derivation fails.
    #[pyo3(name = "address")]
    pub fn py_address(&self) -> PyResult<String> {
        let account = self.inner.account_offline().map_err(to_pyruntime_err)?;
        Ok(account.address)
    }

    fn __repr__(&self) -> String {
        "DydxWallet(<redacted>)".to_string()
    }
}
