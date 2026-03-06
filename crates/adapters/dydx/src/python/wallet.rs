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
